use zabor_profile_helper::TrustedFile;

fn main() {
    let path = std::env::args_os().nth(1).expect("runtime path required");
    match TrustedFile::offline_signed(std::path::Path::new(&path)) {
        Ok(runtime) => println!(
            "offline_authenticode=verified sha256={}",
            runtime
                .sha256()
                .iter()
                .map(|byte| format!("{byte:02x}"))
                .collect::<String>()
        ),
        Err(error) => {
            eprintln!("offline_authenticode=unverified reason={error:?}");
            std::process::exit(1);
        }
    }
}
