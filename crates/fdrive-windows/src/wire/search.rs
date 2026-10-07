use std::ffi::c_void;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use fdrive_core::sdk::Sdk;
use windows::core::{implement, Error, IUnknown, Interface, Ref, Result, BOOL, GUID, PCWSTR};
use windows::Win32::Foundation::{E_NOINTERFACE, E_POINTER, HWND};
use windows::Win32::System::Com::{
    CoDecrementMTAUsage, CoIncrementMTAUsage, CoRegisterClassObject, CoRevokeClassObject, IClassFactory, IClassFactory_Impl,
    IStream, CLSCTX_LOCAL_SERVER, CO_MTA_USAGE_COOKIE, REGCLS_MULTIPLEUSE,
};
use windows::Win32::UI::Shell::{IOpenSearchSource, IOpenSearchSource_Impl, SHCreateMemStream};

pub struct Registration {
    cookie: u32,
    mta: CO_MTA_USAGE_COOKIE,
}

pub fn register(root: &Path, sdk: Arc<Sdk>) -> io::Result<Registration> {
    let root = std::path::absolute(root)?;
    let connector = connector();
    let inside = format!("System.ItemPathDisplay:~<\"{}\"", root.display());
    let open_search = format!("explorer.exe \"search-ms:query=&crumb=location:{}\"", connector.display());
    std::fs::write(&connector, CONNECTOR)?;
    set(SEARCH_KEY, "MUIVerb", "Search in Filestash")?;
    set(SEARCH_KEY, "AppliesTo", &inside)?;
    set(&format!(r"{SEARCH_KEY}\command"), "", &open_search)?;
    set(REVEAL_KEY, "MUIVerb", "Show in folder")?;
    set(REVEAL_KEY, "AppliesTo", &inside)?;
    set(&format!(r"{REVEAL_KEY}\command"), "", r#"explorer.exe /select,"%1""#)?;

    let rt = tokio::runtime::Handle::current();
    let search = move |query: &str| -> Vec<PathBuf> {
        let hits = rt.block_on(sdk.search("/", query)).inspect_err(|err| log::warn!("search: {err}")).unwrap_or_default();
        hits.into_iter().map(|(path, _)| root.join(path.trim_matches('/').replace('/', "\\"))).collect()
    };
    let factory: IClassFactory = Factory(Arc::new(search)).into();
    let mta = unsafe { CoIncrementMTAUsage() }.map_err(io::Error::other)?;
    let cookie = unsafe { CoRegisterClassObject(&CLSID, &factory, CLSCTX_LOCAL_SERVER, REGCLS_MULTIPLEUSE) };
    Ok(Registration { cookie: cookie.map_err(io::Error::other)?, mta })
}

pub fn unregister() {
    let _ = windows_registry::CURRENT_USER.remove_tree(SEARCH_KEY);
    let _ = windows_registry::CURRENT_USER.remove_tree(REVEAL_KEY);
    let _ = windows_registry::CURRENT_USER.remove_tree(CLASS_KEY);
    let _ = std::fs::remove_file(connector());
}

impl Drop for Registration {
    fn drop(&mut self) {
        unsafe {
            let _ = CoRevokeClassObject(self.cookie);
            let _ = CoDecrementMTAUsage(self.mta);
        }
        unregister();
    }
}

type Search = Arc<dyn Fn(&str) -> Vec<PathBuf> + Send + Sync>;

#[implement(IClassFactory)]
struct Factory(Search);

impl IClassFactory_Impl for Factory_Impl {
    fn CreateInstance(&self, outer: Ref<IUnknown>, riid: *const GUID, ppv: *mut *mut c_void) -> Result<()> {
        if outer.is_some() {
            return Err(E_NOINTERFACE.into());
        }
        let source: IOpenSearchSource = Source(self.0.clone()).into();
        unsafe { source.query(&*riid, ppv).ok() }
    }

    fn LockServer(&self, _lock: BOOL) -> Result<()> {
        Ok(())
    }
}

#[implement(IOpenSearchSource)]
struct Source(Search);

impl IOpenSearchSource_Impl for Source_Impl {
    fn GetResults(&self, _: HWND, q: &PCWSTR, start: u32, count: u32, riid: *const GUID, ppv: *mut *mut c_void) -> Result<()> {
        let query = unsafe { q.to_string() }.unwrap_or_default();
        let hits = match (start, query.trim()) {
            (0, query) if !query.is_empty() => (self.0)(query),
            _ => Vec::new(),
        };
        log::debug!("search query={query:?} start={start} count={count} hits={}", hits.len());
        let escape = |s: &str| s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;");
        let items: String = hits.iter().take(count as usize).map(|path| {
            let name = path.file_name().unwrap_or_default().to_string_lossy();
            format!("<item><title>{}</title><link>{}</link></item>", escape(&name), escape(&path.display().to_string()))
        }).collect();
        let rss = format!("{RSS}{items}</channel></rss>");
        let stream: IStream = unsafe { SHCreateMemStream(Some(rss.as_bytes())) }.ok_or(Error::from(E_POINTER))?;
        unsafe { stream.query(&*riid, ppv).ok() }
    }
}

fn connector() -> PathBuf {
    PathBuf::from(std::env::var("USERPROFILE").unwrap_or_default()).join(r"Searches\Filestash.searchConnector-ms")
}

fn set(key: &str, name: &str, value: &str) -> io::Result<()> {
    windows_registry::CURRENT_USER.create(key).and_then(|key| key.set_string(name, value)).map_err(io::Error::other)
}

const CLSID: GUID = GUID::from_u128(0x8fbcf2b1_bfa0_44f5_8e97_17a1e19aea80);
const CLASS_KEY: &str = r"Software\Classes\CLSID\{8FBCF2B1-BFA0-44F5-8E97-17A1E19AEA80}";
const SEARCH_KEY: &str = r"Software\Classes\Directory\shell\FilestashSearch";
const REVEAL_KEY: &str = r"Software\Classes\AllFilesystemObjects\shell\FilestashReveal";
const CONNECTOR: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<searchConnectorDescription xmlns="http://schemas.microsoft.com/windows/2009/searchConnector">
  <description>Search Filestash</description>
  <isSearchOnlyItem>true</isSearchOnlyItem>
  <supportsAdvancedQuerySyntax>false</supportsAdvancedQuerySyntax>
  <templateInfo>
    <folderType>{8FAF9629-1980-46FF-8023-9DCEAB9C3EE3}</folderType>
  </templateInfo>
  <locationProvider clsid="{48E277F6-4E74-4cd6-BA6F-FA4F42898223}">
    <propertyBag>
      <property name="DataSourceCLSID"><![CDATA[{8FBCF2B1-BFA0-44F5-8E97-17A1E19AEA80}]]></property>
      <property name="LinkIsFilePath" type="boolean"><![CDATA[true]]></property>
      <property name="OpenSearchShortName"><![CDATA[Filestash]]></property>
    </propertyBag>
  </locationProvider>
</searchConnectorDescription>
"#;
const RSS: &str = r#"<?xml version="1.0" encoding="UTF-8"?><rss version="2.0"><channel><title>Filestash</title>"#;
