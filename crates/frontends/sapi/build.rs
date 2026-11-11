//! Gives the Windows DLL a version resource. Code signing needs it: SignPath only signs files whose
//! product name and version match what the signing policy expects, and Explorer shows them too.

use std::{env, fs, io, path::PathBuf, process::Command};

fn main() -> io::Result<()> {
    println!("cargo:rerun-if-changed=build.rs");
    let windows = env::var("CARGO_CFG_TARGET_OS").is_ok_and(|os| os == "windows");
    let msvc = env::var("CARGO_CFG_TARGET_ENV").is_ok_and(|target_env| target_env == "msvc");
    if !(windows && msvc) {
        return Ok(());
    }
    let version = env::var("CARGO_PKG_VERSION").map_err(io::Error::other)?;
    let out_dir = env::var_os("OUT_DIR").ok_or_else(|| io::Error::other("OUT_DIR is not set"))?;
    let out = PathBuf::from(out_dir);
    let script = out.join("version.rc");
    let compiled = out.join("version.res");
    fs::write(&script, resource_script(&version))?;
    // rc.exe is on PATH in a Visual Studio developer prompt, which is how CI builds the DLL.
    match Command::new("rc.exe")
        .arg("/nologo")
        .arg("/fo")
        .arg(&compiled)
        .arg(&script)
        .status()
    {
        Ok(status) if status.success() => {
            println!("cargo:rustc-link-arg-cdylib={}", compiled.display());
            Ok(())
        }
        Ok(status) => Err(io::Error::other(format!("rc.exe failed with {status}"))),
        Err(error) => {
            println!(
                "cargo:warning=dengjen_sapi.dll gets no version resource: rc.exe could not run \
                 ({error}); build from a Visual Studio developer prompt to add it"
            );
            Ok(())
        }
    }
}

/// `3.1.0-beta.2` becomes `3,1,0,0`: the numeric form VERSIONINFO's fixed fields take.
fn numeric_version(version: &str) -> String {
    let release = version.split(['-', '+']).next().unwrap_or(version);
    let mut parts: Vec<&str> = release.split('.').take(4).collect();
    parts.resize(4, "0");
    parts.join(",")
}

fn resource_script(version: &str) -> String {
    let numeric = numeric_version(version);
    format!(
        r#"1 VERSIONINFO
FILEVERSION {numeric}
PRODUCTVERSION {numeric}
FILEOS 0x40004
FILETYPE 0x2
BEGIN
  BLOCK "StringFileInfo"
  BEGIN
    BLOCK "040904B0"
    BEGIN
      VALUE "CompanyName", "Zirek"
      VALUE "FileDescription", "Dengjen SAPI5 voice"
      VALUE "FileVersion", "{version}"
      VALUE "InternalName", "dengjen_sapi"
      VALUE "OriginalFilename", "dengjen_sapi.dll"
      VALUE "ProductName", "Dengjen"
      VALUE "ProductVersion", "{version}"
    END
  END
  BLOCK "VarFileInfo"
  BEGIN
    VALUE "Translation", 0x409, 1200
  END
END
"#
    )
}
