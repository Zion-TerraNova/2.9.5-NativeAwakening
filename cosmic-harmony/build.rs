//! Build script for native library linking

fn main() {
    // Path to native libraries
    let native_libs_path = std::env::current_dir()
        .unwrap()
        .parent()
        .unwrap()
        .join("native-libs");
    
    println!("cargo:rustc-link-search=native={}", native_libs_path.display());
    
    // Only link if feature enabled
    #[cfg(feature = "native-randomx")]
    {
        #[cfg(target_os = "macos")]
        println!("cargo:rustc-link-lib=dylib=randomx_zion");
        
        #[cfg(target_os = "linux")]
        println!("cargo:rustc-link-lib=randomx_zion");
    }
    
    #[cfg(feature = "native-yescrypt")]
    {
        #[cfg(target_os = "macos")]
        println!("cargo:rustc-link-lib=dylib=yescrypt_zion");
        
        #[cfg(target_os = "linux")]
        println!("cargo:rustc-link-lib=yescrypt_zion");
    }
    
    #[cfg(feature = "native-cosmic-harmony")]
    {
        #[cfg(target_os = "macos")]
        println!("cargo:rustc-link-lib=dylib=cosmic_harmony_zion");
        
        #[cfg(target_os = "linux")]
        println!("cargo:rustc-link-lib=cosmic_harmony_zion");
    }
    
    #[cfg(feature = "native-autolykos")]
    {
        #[cfg(target_os = "macos")]
        println!("cargo:rustc-link-lib=dylib=autolykos_zion");
        
        #[cfg(target_os = "linux")]
        println!("cargo:rustc-link-lib=autolykos_zion");
    }
    
    #[cfg(feature = "native-kawpow")]
    {
        #[cfg(target_os = "macos")]
        println!("cargo:rustc-link-lib=dylib=kawpow_zion");
        
        #[cfg(target_os = "linux")]
        println!("cargo:rustc-link-lib=kawpow_zion");
    }
    
    #[cfg(feature = "native-kawpow-gpu")]
    {
        #[cfg(target_os = "macos")]
        println!("cargo:rustc-link-lib=dylib=kawpow_gpu_zion");
        
        #[cfg(target_os = "linux")]
        println!("cargo:rustc-link-lib=kawpow_gpu_zion");
    }
    
    #[cfg(feature = "native-ethash")]
    {
        #[cfg(target_os = "macos")]
        println!("cargo:rustc-link-lib=dylib=ethash_zion");
        
        #[cfg(target_os = "linux")]
        println!("cargo:rustc-link-lib=ethash_zion");
    }
    
    #[cfg(feature = "native-kheavyhash")]
    {
        #[cfg(target_os = "macos")]
        println!("cargo:rustc-link-lib=dylib=kheavyhash_zion");
        
        #[cfg(target_os = "linux")]
        println!("cargo:rustc-link-lib=kheavyhash_zion");
    }
    
    #[cfg(feature = "native-equihash")]
    {
        #[cfg(target_os = "macos")]
        println!("cargo:rustc-link-lib=dylib=equihash_zion");
        
        #[cfg(target_os = "linux")]
        println!("cargo:rustc-link-lib=equihash_zion");
    }
    
    #[cfg(feature = "native-progpow")]
    {
        #[cfg(target_os = "macos")]
        println!("cargo:rustc-link-lib=dylib=progpow_zion");
        
        #[cfg(target_os = "linux")]
        println!("cargo:rustc-link-lib=progpow_zion");
    }
    
    #[cfg(feature = "native-argon2d")]
    {
        #[cfg(target_os = "macos")]
        println!("cargo:rustc-link-lib=dylib=argon2d_zion");
        
        #[cfg(target_os = "linux")]
        println!("cargo:rustc-link-lib=argon2d_zion");
    }
    
    #[cfg(feature = "native-blake3")]
    {
        #[cfg(target_os = "macos")]
        println!("cargo:rustc-link-lib=dylib=blake3_zion");
        
        #[cfg(target_os = "linux")]
        println!("cargo:rustc-link-lib=blake3_zion");
    }
    
    // Rerun if native-libs changes
    println!("cargo:rerun-if-changed=../native-libs/");
}
