fn main() {
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
