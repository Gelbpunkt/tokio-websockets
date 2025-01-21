use std::{env, fs, path::PathBuf, process::Command};

fn main() {
    let Ok(rustc) = env::var("RUSTC") else { return };
    let Ok(target_arch) = env::var("CARGO_CFG_TARGET_ARCH") else {
        return;
    };
    let Ok(out_dir) = env::var("OUT_DIR") else {
        return;
    };
    let Ok(target) = env::var("TARGET") else {
        return;
    };
    let Ok(rustflags) = env::var("CARGO_ENCODED_RUSTFLAGS") else {
        return;
    };

    let flags = match target_arch.as_str() {
        "x86_64" => &["stdarch_x86_avx512", "avx512_target_feature"][..],
        "arm" => &[
            "stdarch_arm_neon_intrinsics",
            "stdarch_arm_feature_detection",
            "arm_target_feature",
        ][..],
        "powerpc" | "powerpc64" => &[
            "stdarch_powerpc",
            "stdarch_powerpc_feature_detection",
            "powerpc_target_feature",
        ][..],
        _ => return, // Unknown/unoptimized architecture
    };

    let test_code = format!("#![feature({})]\nfn main() {{}}", flags.join(", "));

    let mut test_file_path = PathBuf::from(&out_dir);
    test_file_path.push("check_flags.rs");

    fs::write(&test_file_path, test_code).expect("failed to write file");

    let result = Command::new(rustc)
        .arg(test_file_path)
        .arg("--out-dir")
        .arg(out_dir)
        .arg("--target")
        .arg(target)
        .args(rustflags.split(char::from(0x1f)))
        .arg("--emit=mir") // This makes it not link, sufficient for checking
        .status();

    match result {
        Ok(status) if status.success() => {
            println!("cargo::rustc-cfg=nightly");
        }
        _ => {}
    }
}
