package app.filestash.sync

import android.content.Context
import android.provider.DocumentsContract
import android.webkit.CookieManager
import app.filestash.core.Adapter
import app.filestash.core.FsException
import app.filestash.core.RemoteObserver
import java.io.File

object Native {
    const val AUTHORITY = "app.filestash.sync.documents"
    const val ROOT_ID = "filestash"

    @Volatile
    var client: Adapter? = null
        private set
    private var store: CredentialStore? = null

    @Synchronized
    fun init(context: Context): CredentialStore {
        val store = store ?: CredentialStore(context.applicationContext).also { store = it }
        if (client == null) {
            store.load()?.let { creds ->
                client = connect(context, creds.url, creds.token)
            }
        }
        return store
    }

    @Synchronized
    fun session(context: Context, url: String, token: String) {
        val store = init(context)
        store.save(Credentials(url, token))
        client?.close()
        client = connect(context, url, token)
        notifyRootsChanged(context)
    }

    @Synchronized
    fun logout(context: Context) {
        init(context).clear()
        CookieManager.getInstance().removeAllCookies(null)
        val old = client
        client = null
        notifyRootsChanged(context)
        if (old != null) {
            Thread {
                try {
                    old.flush(10_000u)
                    old.logout()
                    android.util.Log.i("fdrive", "logout: session invalidated")
                } catch (e: Exception) {
                    android.util.Log.e("fdrive", "logout", e)
                } finally {
                    old.close()
                }
            }.start()
        }
    }

    fun <T> withReauth(context: Context, block: (Adapter) -> T): T {
        init(context)
        return block(client ?: throw FsException.NotAuthenticated())
    }

    private fun connect(context: Context, url: String, token: String): Adapter {
        val resolver = context.applicationContext.contentResolver
        return Adapter(dataDir(context), url, token).also {
            it.startWatch(object : RemoteObserver {
                override fun changed(directories: List<String>) {
                    for (dir in directories) {
                        resolver.notifyChange(DocumentsContract.buildChildDocumentsUri(AUTHORITY, dir), null)
                    }
                }
            })
        }
    }

    private fun dataDir(context: Context): String =
        File(context.applicationContext.filesDir, "fdrive").absolutePath

    fun notifyRootsChanged(context: Context) {
        context.contentResolver.notifyChange(DocumentsContract.buildRootsUri(AUTHORITY), null)
    }
}
