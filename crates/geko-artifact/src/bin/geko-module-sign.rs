//! Signs the packages of the modules before they are published (`ci/modules.sh publish`), with the
//! key of the country packs.
//!
//! ```text
//! geko-module-sign sign <package folder>    writes signature.json with the key in GEKO_PACK_SIGNING_KEY (hex)
//! geko-module-sign verify <package folder>
//! geko-module-sign contract                 the contract of this geko: the tag contract-<n>
//! geko-module-sign platform                 the platform it runs on (linux-x86_64)
//! ```

use std::path::Path;
use std::process::ExitCode;

use geko_artifact::signature::{self, MODULE};

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let result = match args.iter().map(String::as_str).collect::<Vec<_>>().as_slice() {
        ["sign", dir] => std::env::var("GEKO_PACK_SIGNING_KEY")
            .map_err(|_| "GEKO_PACK_SIGNING_KEY is not set".to_owned())
            .and_then(|key| signature::signing_key(&key).map_err(|e| e.to_string()))
            .and_then(|key| signature::sign(Path::new(dir), MODULE, &key).map_err(|e| e.to_string())),
        ["verify", dir] => signature::verify_trusted(Path::new(dir), MODULE).map_err(|e| e.to_string()),
        ["contract"] => {
            println!("{}", geko_module_sdk::module::CONTRACT);
            Ok(())
        }
        ["platform"] => {
            println!("{}", signature::platform());
            Ok(())
        }
        _ => Err("usage: geko-module-sign sign <package folder> | verify <package folder> | contract | platform".to_owned()),
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("{e}");
            ExitCode::FAILURE
        }
    }
}
