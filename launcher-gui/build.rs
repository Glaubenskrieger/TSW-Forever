/// Windows only (MSVC): put the project icon into the executable as a resource, so that Explorer, the taskbar, the Start menu and the shortcuts show it
/// without any external image file. The binary resource file (`.res`) is written directly - its format is a plain sequence of aligned entries - and handed
/// to the MSVC linker, which needs neither `rc.exe` nor an additional crate.
fn embed_windows_icon() {
    use std::io::Write;
    println!("cargo:rerun-if-changed=assets/tsw-forever.ico");
    let ico = std::fs::read("assets/tsw-forever.ico").expect("assets/tsw-forever.ico");
    let u16at = |o: usize| u16::from_le_bytes([ico[o], ico[o + 1]]);
    let u32at = |o: usize| u32::from_le_bytes([ico[o], ico[o + 1], ico[o + 2], ico[o + 3]]);
    assert!(u16at(0) == 0 && u16at(2) == 1, "not an .ico file");
    let count = u16at(4) as usize;
    let mut res: Vec<u8> = Vec::new();
    // one resource entry: DataSize, HeaderSize, TYPE, NAME (numeric ids), DataVersion, MemoryFlags, LanguageId, Version, Characteristics, data (4-byte aligned)
    let entry = |res: &mut Vec<u8>, kind: u16, id: u16, data: &[u8]| {
        res.extend((data.len() as u32).to_le_bytes());
        res.extend(32u32.to_le_bytes());
        res.extend([0xFF, 0xFF]);
        res.extend(kind.to_le_bytes());
        res.extend([0xFF, 0xFF]);
        res.extend(id.to_le_bytes());
        res.extend(0u32.to_le_bytes());
        res.extend(0x1030u16.to_le_bytes()); // MOVEABLE | PURE | DISCARDABLE, as rc.exe writes icons
        res.extend(0x0409u16.to_le_bytes());
        res.extend(0u32.to_le_bytes());
        res.extend(0u32.to_le_bytes());
        res.extend_from_slice(data);
        while !res.len().is_multiple_of(4) {
            res.push(0);
        }
    };
    entry(&mut res, 0, 0, &[]); // the mandatory empty first entry (its header is exactly 32 bytes)
    let mut group = vec![0, 0, 1, 0];
    group.extend((count as u16).to_le_bytes());
    for i in 0..count {
        let e = 6 + 16 * i;
        let (size, offset) = (u32at(e + 8) as usize, u32at(e + 12) as usize);
        let id = (i + 1) as u16;
        entry(&mut res, 3, id, &ico[offset..offset + size]); // RT_ICON
        group.extend_from_slice(&ico[e..e + 8]); // width, height, colours, reserved, planes, bit count
        group.extend((size as u32).to_le_bytes());
        group.extend(id.to_le_bytes());
    }
    entry(&mut res, 14, 1, &group); // RT_GROUP_ICON with the lowest id: Explorer and the taskbar use it as the application icon
    let path = std::path::Path::new(&std::env::var("OUT_DIR").unwrap()).join("tsw-forever-icon.res");
    std::fs::File::create(&path).and_then(|mut f| f.write_all(&res)).expect("write the icon resource");
    println!("cargo:rustc-link-arg-bins={}", path.display());
}

fn main() {
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") && std::env::var("CARGO_CFG_TARGET_ENV").as_deref() == Ok("msvc") {
        embed_windows_icon();
    }
    // Dark standard widgets (text fields) to match the launcher's own design; all other controls are drawn by us.
    // Texts use @tr("English text"); the German (and any later) translation lives in translations/<lang>/LC_MESSAGES/tsw-launcher-gui.po
    // and is bundled into the program, switchable at run time. The English text is the message id, so no default context.
    let config = slint_build::CompilerConfiguration::new()
        .with_style("fluent-dark".into())
        .with_bundled_translations("translations")
        .with_default_translation_context(slint_build::DefaultTranslationContext::None);
    println!("cargo:rerun-if-changed=translations");
    println!("cargo:rerun-if-changed=assets");
    slint_build::compile_with_config("ui/launcher.slint", config).expect("Slint UI compilation failed");
}
