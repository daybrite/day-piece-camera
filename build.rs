// Copyright © The Daybrite Project
// SPDX-License-Identifier: MPL-2.0

//! Compile the HarmonyOS camera shim (ohos/native/day_camera.cpp) with the OpenHarmony NDK's clang
//! and link the camera and image libraries. Only for a `*-linux-ohos` target with the `arkui`
//! feature; a no-op everywhere else, so a host build never needs the NDK. The NDK path comes from
//! `OHOS_NDK_HOME` (the SDK's `native` directory), which the `day` CLI sets when it builds the
//! HarmonyOS target — the same variable day-arkui-sys reads.

fn main() {
    println!("cargo:rerun-if-changed=ohos/native/day_camera.cpp");
    println!("cargo:rerun-if-env-changed=OHOS_NDK_HOME");
    let target_env = std::env::var("CARGO_CFG_TARGET_ENV").unwrap_or_default();
    if target_env == "ohos" && std::env::var("CARGO_FEATURE_ARKUI").is_ok() {
        #[cfg(feature = "arkui")]
        ohos::build();
    }
}

#[cfg(feature = "arkui")]
mod ohos {
    use std::path::PathBuf;

    pub fn build() {
        let ndk = std::env::var("OHOS_NDK_HOME").unwrap_or_else(|_| {
            panic!(
                "day-piece-camera: set OHOS_NDK_HOME to the OpenHarmony NDK `native` dir \
                 (e.g. .../ohos-sdk/native) to build the HarmonyOS arm"
            )
        });
        let ndk = PathBuf::from(ndk);
        let target = std::env::var("TARGET").unwrap_or_default();
        let arch = std::env::var("CARGO_CFG_TARGET_ARCH").unwrap_or_default();
        // clang++: the camera headers reach `rawfile/raw_file.h`, which is C++-only in the SDK.
        let clang = ndk.join("llvm/bin").join(format!("{target}-clang++"));
        let ar = ndk.join("llvm/bin/llvm-ar");
        let include = ndk.join("sysroot/usr/include");
        cc::Build::new()
            .cpp(true)
            .compiler(&clang)
            .archiver(&ar)
            .flag("-std=c++17")
            .flag("-fPIC")
            .include(&include)
            .file("ohos/native/day_camera.cpp")
            .compile("day_camera_ohos");
        let lib_arch = match arch.as_str() {
            "aarch64" => "aarch64-linux-ohos",
            "x86_64" => "x86_64-linux-ohos",
            "arm" => "arm-linux-ohos",
            other => panic!("day-piece-camera: unsupported OHOS arch {other}"),
        };
        let libdir = ndk.join("sysroot/usr/lib").join(lib_arch);
        println!("cargo:rustc-link-search=native={}", libdir.display());
        // The camera kit, the native image the photo output hands over, and the buffer it is read
        // through.
        for lib in ["ohcamera", "ohimage", "native_buffer"] {
            println!("cargo:rustc-link-lib=dylib={lib}");
        }
    }
}
