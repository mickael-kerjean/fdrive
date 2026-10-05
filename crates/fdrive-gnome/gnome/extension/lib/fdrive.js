import Gio from "gi://Gio";

Gio._promisify(Gio.Subprocess.prototype, "communicate_utf8_async");

const OFF = {phase: "stopped", phaseText: "Off", server: "", host: "", mount: "", lastError: "", transfers: [], sparkline: "", rate: ""};

async function run(argv, input = null) {
    const flags = Gio.SubprocessFlags.STDIN_PIPE | Gio.SubprocessFlags.STDOUT_PIPE | Gio.SubprocessFlags.STDERR_PIPE;
    const proc = Gio.Subprocess.new(argv, flags);
    const [out, err] = await proc.communicate_utf8_async(input, null);
    if (!proc.get_successful())
        throw new Error(err.trim().split("\n")[0].replace(/^fdrive: /, "") || `${argv.join(" ")} failed`);
    return out;
}

export async function status() {
    try {
        return {...OFF, ...JSON.parse(await run(["fdrive", "status"]))};
    } catch {
        return OFF;
    }
}

export async function loginUrl(host) {
    return (await run(["fdrive", "login-url", host])).trim();
}

export function login(host, token) {
    return run(["fdrive", "login", host], `${token}\n`);
}

export function logout() {
    return run(["fdrive", "logout"]);
}

export function clear() {
    return run(["fdrive", "clear"]);
}

export function setActive(on) {
    return run(["systemctl", "--user", on ? "start" : "stop", "fdrive"]);
}
