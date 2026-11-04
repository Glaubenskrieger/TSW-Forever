# Third-party notices

The complete collected Linux distribution texts and font notices are in [`LICENSES/`](LICENSES/README.md), also installed under `/app/share/licenses/tsw-forever/`. Its pinned inventory covers 325 Linux normal/build registry dependencies; this table conservatively lists all 542 cross-platform lockfile entries, not 542 linked libraries. Original licence/copyright texts control over the abbreviated author column below.

TSW-Forever is licensed under GPL-3.0-or-later (see LICENSE). The binary packages (Flatpak) contain the Rust libraries listed below, each under its own licence. This list is generated from the lock file of the GUI crate (which includes the launcher and patcher dependencies) with `cargo metadata`; where a crate offers several licences ("A OR B") the project relies on the GPL-compatible choice stated at the end. The full licence texts are part of each crate's source package (crates.io, or `cargo vendor`), which also carries its copyright notices.

**Slint** (GUI toolkit, crates `slint`, `i-slint-*`): offered under `GPL-3.0-only OR LicenseRef-Slint-Royalty-free-2.0 OR LicenseRef-Slint-Software-3.0`; **used under GPL-3.0-only**. The combined binary is therefore distributed under the terms of the GPL version 3.

**Not part of the package** (downloaded by the user on request, not redistributed by this project): GE-Proton / Wine / DXVK / vkd3d-proton (their own licences), Microsoft DirectX components, the Funcom installer and game.

| Crate | Version | Licence | Authors |
|---|---|---|---|
| ab_glyph | 0.2.32 | Apache-2.0 | Alex Butler |
| ab_glyph_rasterizer | 0.1.10 | Apache-2.0 | Alex Butler |
| adler2 | 2.0.1 | 0BSD OR MIT OR Apache-2.0 | Jonas Schievink, oyvindln |
| ahash | 0.8.12 | MIT OR Apache-2.0 | Tom Kaitchuck |
| aho-corasick | 1.1.5 | Unlicense OR MIT | Andrew Gallant |
| allocator-api2 | 0.2.21 | MIT OR Apache-2.0 | Zakarum |
| android-activity | 0.6.1 | MIT OR Apache-2.0 | - |
| android-properties | 0.2.2 | MIT | Mikhail Lappo |
| android_system_properties | 0.1.6 | MIT OR Apache-2.0 | Nicolas Silva |
| annotate-snippets | 0.12.16 | MIT OR Apache-2.0 | - |
| anstyle | 1.0.14 | MIT OR Apache-2.0 | - |
| arboard | 3.6.1 | MIT OR Apache-2.0 | - |
| arrayref | 0.3.9 | BSD-2-Clause | David Roundy |
| arrayvec | 0.7.8 | MIT OR Apache-2.0 | bluss |
| as-raw-xcb-connection | 1.0.1 | MIT OR Apache-2.0 | - |
| ash | 0.38.0+1.3.281 | MIT OR Apache-2.0 | Maik Klein, Benjamin Saunders, Marijn Suijten |
| async-broadcast | 0.7.2 | MIT OR Apache-2.0 | Stjepan Glavina, Yoshua Wuyts, Zeeshan Ali Khan |
| async-channel | 2.5.0 | Apache-2.0 OR MIT | Stjepan Glavina |
| async-executor | 1.14.0 | Apache-2.0 OR MIT | Stjepan Glavina, John Nunley |
| async-io | 2.6.0 | Apache-2.0 OR MIT | Stjepan Glavina |
| async-lock | 3.4.2 | Apache-2.0 OR MIT | Stjepan Glavina |
| async-process | 2.5.0 | Apache-2.0 OR MIT | Stjepan Glavina |
| async-recursion | 1.1.1 | MIT OR Apache-2.0 | Robert Usher |
| async-signal | 0.2.14 | Apache-2.0 OR MIT | John Nunley |
| async-task | 4.7.1 | Apache-2.0 OR MIT | Stjepan Glavina |
| async-trait | 0.1.92 | MIT OR Apache-2.0 | David Tolnay |
| atomic-waker | 1.1.2 | Apache-2.0 OR MIT | Stjepan Glavina, Contributors to futures-rs |
| autocfg | 1.5.1 | Apache-2.0 OR MIT | Josh Stone |
| base64 | 0.23.1 | MIT OR Apache-2.0 | Marshall Pierce |
| bincode | 2.0.1 | MIT | Ty Overby, Zoey Riordan, Victor Koenders |
| bindgen | 0.72.1 | BSD-3-Clause | Jyun-Yan You, Emilio Cobos Álvarez, Nick Fitzgerald, The Servo project developer |
| bit-set | 0.10.0 | Apache-2.0 OR MIT | Alexis Beingessner |
| bit-vec | 0.9.1 | Apache-2.0 OR MIT | Alexis Beingessner |
| bitflags | 1.3.2 | MIT/Apache-2.0 | The Rust Project Developers |
| bitflags | 2.13.2 | MIT OR Apache-2.0 | The Rust Project Developers |
| block2 | 0.5.1 | MIT | Steven Sheldon, Mads Marquart |
| block2 | 0.6.2 | MIT | Mads Marquart |
| blocking | 1.7.0 | Apache-2.0 OR MIT | - |
| borsh | 1.8.1 | MIT OR Apache-2.0 | Near Inc |
| bumpalo | 3.20.3 | MIT OR Apache-2.0 | Nick Fitzgerald |
| by_address | 1.2.1 | MIT OR Apache-2.0 | Matt Brubeck |
| bytemuck | 1.25.2 | Zlib OR Apache-2.0 OR MIT | Lokathor |
| bytemuck_derive | 1.12.1 | Zlib OR Apache-2.0 OR MIT | Lokathor |
| byteorder-lite | 0.1.0 | Unlicense OR MIT | - |
| bytes | 1.12.1 | MIT | Carl Lerche, Sean McArthur |
| calloop | 0.13.0 | MIT | Elinor Berger |
| calloop | 0.14.5 | MIT | Elinor Berger |
| calloop-wayland-source | 0.3.0 | MIT | Kirill Chibisov |
| cc | 1.5.1 | MIT OR Apache-2.0 | - |
| cexpr | 0.6.0 | Apache-2.0/MIT | Jethro Beekman |
| cfg-if | 1.0.5 | MIT OR Apache-2.0 | Alex Crichton |
| cfg_aliases | 0.2.2 | MIT | Zicklag |
| cgl | 0.3.2 | MIT / Apache-2.0 | The Servo Project Developers |
| chrono | 0.4.45 | MIT OR Apache-2.0 | - |
| clang-sys | 1.9.1 | Apache-2.0 | Kyle Mayes |
| clipboard-win | 5.4.1 | BSL-1.0 | Douman |
| clru | 0.6.3 | MIT | marmeladema |
| codespan-reporting | 0.13.1 | Apache-2.0 | Brendan Zabarauskas |
| color_quant | 1.1.0 | MIT | nwin |
| combine | 4.6.8 | MIT | Markus Westerlind |
| concurrent-queue | 2.5.0 | Apache-2.0 OR MIT | Stjepan Glavina, Taiki Endo, John Nunley |
| const-field-offset | 0.2.1 | MIT OR Apache-2.0 | Slint Developers |
| const-field-offset-macro | 0.2.1 | MIT OR Apache-2.0 | Slint Developers |
| convert_case | 0.10.0 | MIT | rutrum |
| core-foundation | 0.9.4 | MIT OR Apache-2.0 | The Servo Project Developers |
| core-foundation-sys | 0.8.7 | MIT OR Apache-2.0 | The Servo Project Developers |
| core-graphics | 0.23.2 | MIT OR Apache-2.0 | The Servo Project Developers |
| core-graphics-types | 0.1.3 | MIT OR Apache-2.0 | The Servo Project Developers |
| countme | 3.0.1 | MIT OR Apache-2.0 | Aleksey Kladov |
| crc32fast | 1.5.2 | MIT OR Apache-2.0 | Sam Rijs, Alex Crichton |
| critical-section | 1.2.0 | MIT OR Apache-2.0 | - |
| crossbeam-channel | 0.5.17 | MIT OR Apache-2.0 | - |
| crossbeam-deque | 0.8.8 | MIT OR Apache-2.0 | - |
| crossbeam-epoch | 0.9.21 | MIT OR Apache-2.0 | - |
| crossbeam-utils | 0.8.23 | MIT OR Apache-2.0 | - |
| crunchy | 0.2.4 | MIT | Eira Fransham |
| ctor | 0.10.1 | Apache-2.0 OR MIT | Matt Mastracci |
| cursor-icon | 1.2.0 | MIT OR Apache-2.0 OR Zlib | Kirill Chibisov |
| data-url | 0.3.2 | MIT OR Apache-2.0 | Simon Sapin |
| derive_more | 2.1.1 | MIT | Jelte Fennema |
| derive_more-impl | 2.1.1 | MIT | Jelte Fennema |
| dispatch | 0.2.0 | MIT | Steven Sheldon |
| dispatch2 | 0.3.1 | Zlib OR Apache-2.0 OR MIT | Mads Marquart, Mary |
| displaydoc | 0.2.7 | MIT OR Apache-2.0 | Jane Lusby |
| dlib | 0.5.3 | MIT | Elinor Berger |
| document-features | 0.2.12 | MIT OR Apache-2.0 | Slint Developers |
| downcast-rs | 1.2.1 | MIT/Apache-2.0 | Ashish Myles, Runji Wang |
| dpi | 0.1.2 | Apache-2.0 AND MIT | - |
| drm | 0.14.1 | MIT | Tyler Slabinski, Victoria Brekenfeld |
| drm-ffi | 0.9.1 | MIT | Tyler Slabinski |
| drm-fourcc | 2.2.0 | MIT | Daniel Franklin |
| drm-sys | 0.8.1 | MIT | Tyler Slabinski |
| dtor | 0.8.1 | Apache-2.0 OR MIT | Matt Mastracci |
| either | 1.18.0 | MIT OR Apache-2.0 | - |
| endi | 1.1.1 | MIT | Zeeshan Ali Khan |
| enumflags2 | 0.7.12 | MIT OR Apache-2.0 | maik klein, Maja Kądziołka |
| enumflags2_derive | 0.7.12 | MIT OR Apache-2.0 | maik klein, Maja Kądziołka |
| equivalent | 1.0.2 | Apache-2.0 OR MIT | - |
| errno | 0.3.14 | MIT OR Apache-2.0 | Chris Wong, Dan Gohman |
| error-code | 3.4.0 | BSL-1.0 | Douman |
| euclid | 0.22.14 | MIT OR Apache-2.0 | The Servo Project Developers |
| event-listener | 5.4.2 | Apache-2.0 OR MIT | Stjepan Glavina, John Nunley |
| event-listener-strategy | 0.5.4 | Apache-2.0 OR MIT | John Nunley |
| fastrand | 2.5.0 | Apache-2.0 OR MIT | Stjepan Glavina |
| fdeflate | 0.3.7 | MIT OR Apache-2.0 | The image-rs Developers |
| field-offset | 0.3.6 | MIT OR Apache-2.0 | Diggory Blake |
| filetime | 0.2.29 | MIT/Apache-2.0 | Alex Crichton |
| find-msvc-tools | 0.1.14 | MIT OR Apache-2.0 | - |
| fixed_decimal | 0.7.2 | Unicode-3.0 | The ICU4X Project Developers |
| fixedbitset | 0.5.7 | MIT OR Apache-2.0 | bluss |
| flate2 | 1.1.10 | MIT OR Apache-2.0 | Alex Crichton, Josh Triplett |
| float-cmp | 0.9.0 | MIT | Mike Dilger |
| foldhash | 0.1.5 | Zlib | Orson Peters |
| foldhash | 0.2.0 | Zlib | Orson Peters |
| font-types | 0.12.5 | MIT OR Apache-2.0 | - |
| fontdb | 0.24.0 | MIT | Yevhenii Reizner |
| fontique | 0.11.1 | Apache-2.0 OR MIT | - |
| foreign-types | 0.5.0 | MIT/Apache-2.0 | Steven Fackler |
| foreign-types-macros | 0.2.4 | MIT/Apache-2.0 | Steven Fackler |
| foreign-types-shared | 0.3.1 | MIT/Apache-2.0 | Steven Fackler |
| form_urlencoded | 1.2.2 | MIT OR Apache-2.0 | The rust-url developers |
| futures | 0.3.34 | MIT OR Apache-2.0 | - |
| futures-channel | 0.3.34 | MIT OR Apache-2.0 | - |
| futures-core | 0.3.34 | MIT OR Apache-2.0 | - |
| futures-executor | 0.3.34 | MIT OR Apache-2.0 | - |
| futures-io | 0.3.34 | MIT OR Apache-2.0 | - |
| futures-lite | 2.6.1 | Apache-2.0 OR MIT | Stjepan Glavina, Contributors to futures-rs |
| futures-macro | 0.3.34 | MIT OR Apache-2.0 | - |
| futures-sink | 0.3.34 | MIT OR Apache-2.0 | - |
| futures-task | 0.3.34 | MIT OR Apache-2.0 | - |
| futures-util | 0.3.34 | MIT OR Apache-2.0 | - |
| gethostname | 1.1.0 | Apache-2.0 | Sebastian Wiesner |
| getrandom | 0.3.4 | MIT OR Apache-2.0 | The Rand Project Developers |
| getrandom | 0.4.3 | MIT OR Apache-2.0 | The Rand Project Developers |
| gif | 0.14.2 | MIT OR Apache-2.0 | The image-rs Developers |
| gl_generator | 0.14.0 | Apache-2.0 | Brendan Zabarauskas, Corey Richardson, Arseny Kapoulkine |
| glob | 0.3.4 | MIT OR Apache-2.0 | The Rust Project Developers |
| glow | 0.18.0 | MIT OR Apache-2.0 OR Zlib | Joshua Groves, Dzmitry Malyshau |
| glutin | 0.32.3 | Apache-2.0 | Kirill Chibisov |
| glutin-winit | 0.5.0 | MIT | Kirill Chibisov |
| glutin_egl_sys | 0.7.1 | Apache-2.0 | Kirill Chibisov |
| glutin_glx_sys | 0.6.1 | Apache-2.0 | Kirill Chibisov |
| glutin_wgl_sys | 0.6.1 | Apache-2.0 | Kirill Chibisov |
| gpu-allocator | 0.28.0 | MIT OR Apache-2.0 | Traverse Research |
| grid | 1.0.1 | MIT | Armin Becher |
| half | 2.7.1 | MIT OR Apache-2.0 | Kathryn Long |
| harfrust | 0.12.0 | MIT | - |
| hashbrown | 0.14.5 | MIT OR Apache-2.0 | Amanieu d'Antras |
| hashbrown | 0.15.5 | MIT OR Apache-2.0 | Amanieu d'Antras |
| hashbrown | 0.16.1 | MIT OR Apache-2.0 | Amanieu d'Antras |
| hashbrown | 0.17.1 | MIT OR Apache-2.0 | - |
| heck | 0.5.0 | MIT OR Apache-2.0 | - |
| hermit-abi | 0.3.9 | MIT OR Apache-2.0 | Stefan Lankes |
| hermit-abi | 0.5.3 | MIT OR Apache-2.0 | - |
| hex | 0.4.3 | MIT OR Apache-2.0 | KokaKiwi |
| htmlparser | 0.2.1 | MIT OR Apache-2.0 | Jeremie Drouet |
| i-slint-backend-linuxkms | 1.18.1 | GPL-3.0-only OR LicenseRef-Slint-Royalty-free-2.0 OR LicenseRef-Slint-Software-3.0 | Slint Developers |
| i-slint-backend-selector | 1.18.1 | GPL-3.0-only OR LicenseRef-Slint-Royalty-free-2.0 OR LicenseRef-Slint-Software-3.0 | Slint Developers |
| i-slint-backend-testing | 1.18.1 | GPL-3.0-only OR LicenseRef-Slint-Royalty-free-2.0 OR LicenseRef-Slint-Software-3.0 | Slint Developers |
| i-slint-backend-winit | 1.18.1 | GPL-3.0-only OR LicenseRef-Slint-Royalty-free-2.0 OR LicenseRef-Slint-Software-3.0 | Slint Developers |
| i-slint-common | 1.18.1 | GPL-3.0-only OR LicenseRef-Slint-Royalty-free-2.0 OR LicenseRef-Slint-Software-3.0 | Slint Developers |
| i-slint-compiler | 1.18.1 | GPL-3.0-only OR LicenseRef-Slint-Royalty-free-2.0 OR LicenseRef-Slint-Software-3.0 | Slint Developers |
| i-slint-core | 1.18.1 | GPL-3.0-only OR LicenseRef-Slint-Royalty-free-2.0 OR LicenseRef-Slint-Software-3.0 | Slint Developers |
| i-slint-core-macros | 1.18.1 | GPL-3.0-only OR LicenseRef-Slint-Royalty-free-2.0 OR LicenseRef-Slint-Software-3.0 | Slint Developers |
| i-slint-renderer-skia | 1.18.1 | GPL-3.0-only OR LicenseRef-Slint-Royalty-free-2.0 OR LicenseRef-Slint-Software-3.0 | Slint Developers |
| i-slint-renderer-software | 1.18.1 | GPL-3.0-only OR LicenseRef-Slint-Royalty-free-2.0 OR LicenseRef-Slint-Software-3.0 | Slint Developers |
| iana-time-zone | 0.1.65 | MIT OR Apache-2.0 | Andrew Straw, René Kijewski, Ryan Lopopolo |
| iana-time-zone-haiku | 0.1.2 | MIT OR Apache-2.0 | René Kijewski |
| icu_collections | 2.3.0 | Unicode-3.0 | The ICU4X Project Developers |
| icu_decimal | 2.3.0 | Unicode-3.0 | The ICU4X Project Developers |
| icu_decimal_data | 2.3.0 | Unicode-3.0 | The ICU4X Project Developers |
| icu_locale_core | 2.3.0 | Unicode-3.0 | The ICU4X Project Developers |
| icu_locale_fallback | 2.3.0 | Unicode-3.0 | The ICU4X Project Developers |
| icu_locale_fallback_data | 2.3.0 | Unicode-3.0 | The ICU4X Project Developers |
| icu_normalizer | 2.3.0 | Unicode-3.0 | The ICU4X Project Developers |
| icu_normalizer_data | 2.3.0 | Unicode-3.0 | The ICU4X Project Developers |
| icu_plurals | 2.3.0 | Unicode-3.0 | The ICU4X Project Developers |
| icu_plurals_data | 2.3.0 | Unicode-3.0 | The ICU4X Project Developers |
| icu_properties | 2.3.0 | Unicode-3.0 | The ICU4X Project Developers |
| icu_properties_data | 2.3.0 | Unicode-3.0 | The ICU4X Project Developers |
| icu_provider | 2.3.1 | Unicode-3.0 | The ICU4X Project Developers |
| icu_segmenter | 2.3.0 | Unicode-3.0 | The ICU4X Project Developers |
| icu_segmenter_data | 2.3.0 | Unicode-3.0 | The ICU4X Project Developers |
| idna | 1.1.0 | MIT OR Apache-2.0 | The rust-url developers |
| idna_adapter | 1.2.2 | Apache-2.0 OR MIT | The rust-url developers |
| image | 0.25.10 | MIT OR Apache-2.0 | The image-rs Developers |
| image-webp | 0.2.4 | MIT OR Apache-2.0 | - |
| imagesize | 0.15.0 | MIT | Maid Dog |
| imgref | 1.12.3 | CC0-1.0 OR Apache-2.0 | Kornel Lesiński |
| indexmap | 2.14.2 | Apache-2.0 OR MIT | - |
| input | 0.10.0 | MIT | Drakulix (Victoria Brekenfeld) |
| input-sys | 1.19.0 | MIT | Victoria Brekenfeld (Drakulix) |
| io-lifetimes | 1.0.11 | Apache-2.0 WITH LLVM-exception OR Apache-2.0 OR MIT | Dan Gohman |
| itertools | 0.13.0 | MIT OR Apache-2.0 | bluss |
| itertools | 0.15.0 | MIT OR Apache-2.0 | bluss |
| itoa | 1.0.18 | MIT OR Apache-2.0 | David Tolnay |
| jni | 0.22.4 | MIT OR Apache-2.0 | jni team |
| jni-macros | 0.22.4 | MIT OR Apache-2.0 | - |
| jni-sys | 0.3.1 | MIT OR Apache-2.0 | Steven Fackler |
| jni-sys | 0.4.1 | MIT OR Apache-2.0 | Steven Fackler, Robert Bragg |
| jni-sys-macros | 0.4.1 | MIT OR Apache-2.0 | Robert Bragg |
| jobserver | 0.1.35 | MIT OR Apache-2.0 | Alex Crichton |
| js-sys | 0.3.106 | MIT OR Apache-2.0 | The wasm-bindgen Developers |
| keyboard-types | 0.7.0 | MIT OR Apache-2.0 | Pyfisch |
| khronos_api | 3.1.0 | Apache-2.0 | Brendan Zabarauskas, Corey Richardson, Arseny Kapoulkine, Pierre Krieger |
| kurbo | 0.13.1 | Apache-2.0 OR MIT | - |
| lazy_static | 1.5.1 | MIT OR Apache-2.0 | Marvin Löbel |
| libc | 0.2.189 | MIT OR Apache-2.0 | - |
| libloading | 0.8.9 | ISC | Simonas Kazlauskas |
| libm | 0.2.16 | MIT | Alex Crichton, Amanieu d'Antras, Jorge Aparicio, Trevor Gross |
| libredox | 0.1.25 | MIT | 4lDO2 |
| libseat | 0.2.4 | MIT | Poly |
| libseat-sys | 0.2.0 | MIT | Poly |
| libudev-sys | 0.1.4 | MIT | David Cuddeback |
| linebender_resource_handle | 0.1.1 | Apache-2.0 OR MIT | - |
| linux-raw-sys | 0.12.1 | Apache-2.0 WITH LLVM-exception OR Apache-2.0 OR MIT | Dan Gohman |
| linux-raw-sys | 0.4.15 | Apache-2.0 WITH LLVM-exception OR Apache-2.0 OR MIT | Dan Gohman |
| linux-raw-sys | 0.9.4 | Apache-2.0 WITH LLVM-exception OR Apache-2.0 OR MIT | Dan Gohman |
| litemap | 0.8.3 | Unicode-3.0 | The ICU4X Project Developers |
| litrs | 1.0.0 | MIT OR Apache-2.0 | Lukas Kalbertodt |
| lock_api | 0.4.14 | MIT OR Apache-2.0 | Amanieu d'Antras |
| log | 0.4.34 | MIT OR Apache-2.0 | The Rust Project Developers |
| lyon_algorithms | 1.0.21 | MIT OR Apache-2.0 | Nicolas Silva |
| lyon_extra | 1.1.0 | MIT OR Apache-2.0 | Nicolas Silva |
| lyon_geom | 1.0.19 | MIT OR Apache-2.0 | Nicolas Silva |
| lyon_path | 1.0.19 | MIT OR Apache-2.0 | Nicolas Silva |
| memchr | 2.8.3 | Unlicense OR MIT | Andrew Gallant, bluss |
| memmap2 | 0.9.11 | MIT OR Apache-2.0 | Dan Burkert, Yevhenii Reizner, The Contributors |
| memoffset | 0.9.1 | MIT | Gilad Naaman |
| minimal-lexical | 0.2.1 | MIT/Apache-2.0 | Alex Huszagh |
| minisign-verify | 0.2.4 | MIT | Frank Denis |
| miniz_oxide | 0.8.9 | MIT OR Zlib OR Apache-2.0 | Frommi, oyvindln, Rich Geldreich richgel99@gmail.com |
| miniz_oxide | 0.9.1 | MIT OR Zlib OR Apache-2.0 | Frommi, oyvindln, Rich Geldreich richgel99@gmail.com |
| moxcms | 0.8.1 | BSD-3-Clause OR Apache-2.0 | Radzivon Bartoshyk |
| muda | 0.19.3 | Apache-2.0 OR MIT | - |
| naga | 30.0.1 | MIT OR Apache-2.0 | gfx-rs developers |
| naga-types | 30.0.1 | MIT OR Apache-2.0 | gfx-rs developers |
| natord | 1.0.9 | MIT | Kang Seonghoon |
| ndk | 0.9.0 | MIT OR Apache-2.0 | The Rust Mobile contributors |
| ndk-context | 0.1.1 | MIT OR Apache-2.0 | The Rust Windowing contributors |
| ndk-sys | 0.6.0+11769913 | MIT OR Apache-2.0 | The Rust Windowing contributors |
| nix | 0.31.3 | MIT | - |
| nom | 7.1.3 | MIT | contact@geoffroycouprie.com |
| nom | 8.0.0 | MIT | contact@geoffroycouprie.com |
| num-traits | 0.2.19 | MIT OR Apache-2.0 | The Rust Project Developers |
| num_enum | 0.7.6 | BSD-3-Clause OR MIT OR Apache-2.0 | Daniel Wagner-Hall, Daniel Henry-Mantilla, Vincent Esche |
| num_enum_derive | 0.7.6 | BSD-3-Clause OR MIT OR Apache-2.0 | Daniel Wagner-Hall, Daniel Henry-Mantilla, Vincent Esche |
| objc-sys | 0.3.5 | MIT | Mads Marquart |
| objc2 | 0.5.2 | MIT | Steven Sheldon, Mads Marquart |
| objc2 | 0.6.4 | MIT | Mads Marquart |
| objc2-app-kit | 0.2.2 | MIT | - |
| objc2-app-kit | 0.3.2 | Zlib OR Apache-2.0 OR MIT | - |
| objc2-cloud-kit | 0.2.2 | MIT | - |
| objc2-contacts | 0.2.2 | MIT | - |
| objc2-core-data | 0.2.2 | MIT | - |
| objc2-core-foundation | 0.3.2 | Zlib OR Apache-2.0 OR MIT | - |
| objc2-core-graphics | 0.3.2 | Zlib OR Apache-2.0 OR MIT | - |
| objc2-core-image | 0.2.2 | MIT | - |
| objc2-core-location | 0.2.2 | MIT | - |
| objc2-core-text | 0.3.2 | Zlib OR Apache-2.0 OR MIT | - |
| objc2-encode | 4.1.0 | MIT | Mads Marquart |
| objc2-foundation | 0.2.2 | MIT | - |
| objc2-foundation | 0.3.2 | MIT | - |
| objc2-io-surface | 0.3.2 | Zlib OR Apache-2.0 OR MIT | - |
| objc2-link-presentation | 0.2.2 | MIT | - |
| objc2-metal | 0.2.2 | MIT | - |
| objc2-metal | 0.3.2 | Zlib OR Apache-2.0 OR MIT | - |
| objc2-quartz-core | 0.2.2 | MIT | - |
| objc2-quartz-core | 0.3.2 | Zlib OR Apache-2.0 OR MIT | - |
| objc2-symbols | 0.2.2 | MIT | - |
| objc2-ui-kit | 0.2.2 | MIT | - |
| objc2-ui-kit | 0.3.2 | Zlib OR Apache-2.0 OR MIT | - |
| objc2-uniform-type-identifiers | 0.2.2 | MIT | - |
| objc2-user-notifications | 0.2.2 | MIT | - |
| once_cell | 1.21.4 | MIT OR Apache-2.0 | Aleksey Kladov |
| orbclient | 0.3.55 | MIT | Jeremy Soller |
| ordered-float | 5.5.0 | MIT | Jonathan Reem, Matt Brubeck |
| ordered-stream | 0.2.0 | MIT OR Apache-2.0 | Daniel De Graaf, Zeeshan Ali Khan |
| os_pipe | 1.2.3 | MIT | Jack O'Connor |
| owned_ttf_parser | 0.25.1 | Apache-2.0 | Alex Butler |
| parking | 2.2.1 | Apache-2.0 OR MIT | Stjepan Glavina, The Rust Project Developers |
| parking_lot | 0.12.5 | MIT OR Apache-2.0 | Amanieu d'Antras |
| parking_lot_core | 0.9.12 | MIT OR Apache-2.0 | Amanieu d'Antras |
| parlance | 0.1.0 | Apache-2.0 OR MIT | - |
| parley | 0.11.1 | Apache-2.0 OR MIT | - |
| parley_data | 0.11.1 | Apache-2.0 OR MIT | - |
| percent-encoding | 2.3.2 | MIT OR Apache-2.0 | The rust-url developers |
| petgraph | 0.8.3 | MIT OR Apache-2.0 | bluss, mitchmindtree |
| pico-args | 0.5.0 | MIT | Yevhenii Reizner |
| pin-project | 1.1.13 | Apache-2.0 OR MIT | - |
| pin-project-internal | 1.1.13 | Apache-2.0 OR MIT | - |
| pin-project-lite | 0.2.17 | Apache-2.0 OR MIT | - |
| pin-utils | 0.1.1 | MIT OR Apache-2.0 | Josef Brandl |
| pin-weak | 1.1.0 | MIT | Olivier Goffart |
| piper | 0.2.5 | MIT OR Apache-2.0 | Stjepan Glavina, John Nunley |
| pkg-config | 0.3.34 | MIT OR Apache-2.0 | Alex Crichton |
| plain | 0.2.3 | MIT/Apache-2.0 | jzr |
| png | 0.18.1 | MIT OR Apache-2.0 | The image-rs Developers |
| polling | 3.11.0 | Apache-2.0 OR MIT | Stjepan Glavina, John Nunley |
| polycool | 0.4.0 | MIT OR Apache-2.0 | - |
| portable-atomic | 1.15.0 | Apache-2.0 OR MIT | - |
| portable-atomic-util | 0.2.8 | Apache-2.0 OR MIT | - |
| potential_utf | 0.1.6 | Unicode-3.0 | The ICU4X Project Developers |
| presser | 0.3.1 | MIT OR Apache-2.0 | Embark, Gray Olson |
| prettyplease | 0.2.37 | MIT OR Apache-2.0 | David Tolnay |
| proc-macro-crate | 3.5.0 | MIT OR Apache-2.0 | Bastian Köcher |
| proc-macro2 | 1.0.107 | MIT OR Apache-2.0 | David Tolnay, Alex Crichton |
| profiling | 1.0.18 | MIT OR Apache-2.0 | Philip Degarmo |
| pulldown-cmark | 0.13.4 | MIT | Raph Levien, Marcus Klaas de Vries |
| pxfm | 0.1.30 | BSD-3-Clause OR Apache-2.0 | Radzivon Bartoshyk |
| quick-error | 2.0.1 | MIT/Apache-2.0 | Paul Colomiets, Colin Kiegel |
| quick-xml | 0.41.0 | MIT | - |
| quote | 1.0.47 | MIT OR Apache-2.0 | David Tolnay |
| r-efi | 5.3.0 | MIT OR Apache-2.0 OR LGPL-2.1-or-later | - |
| r-efi | 6.0.0 | MIT OR Apache-2.0 OR LGPL-2.1-or-later | - |
| range-alloc | 0.1.5 | MIT OR Apache-2.0 | the gfx-rs Developers |
| raw-window-handle | 0.6.2 | MIT OR Apache-2.0 OR Zlib | Osspial |
| raw-window-metal | 1.1.0 | MIT OR Apache-2.0 | - |
| rayon | 1.12.0 | MIT OR Apache-2.0 | - |
| rayon-core | 1.13.0 | MIT OR Apache-2.0 | - |
| read-fonts | 0.41.0 | MIT OR Apache-2.0 | - |
| redox_syscall | 0.4.1 | MIT | Jeremy Soller |
| redox_syscall | 0.5.18 | MIT | Jeremy Soller |
| redox_syscall | 0.9.4 | MIT | Jeremy Soller |
| regex | 1.13.1 | MIT OR Apache-2.0 | The Rust Project Developers, Andrew Gallant |
| regex-automata | 0.4.18 | MIT OR Apache-2.0 | The Rust Project Developers, Andrew Gallant |
| regex-syntax | 0.8.11 | MIT OR Apache-2.0 | The Rust Project Developers, Andrew Gallant |
| renderdoc-sys | 1.1.0 | MIT OR Apache-2.0 | Eyal Kalderon |
| resvg | 0.48.1 | Apache-2.0 OR MIT | - |
| rgb | 0.8.53 | MIT | Kornel Lesiński, James Forster |
| rowan | 0.17.0 | MIT OR Apache-2.0 | Aleksey Kladov |
| roxmltree | 0.21.1 | MIT OR Apache-2.0 | Yevhenii Reizner |
| rspolib | 0.1.2 | MIT | - |
| rustc-hash | 1.1.0 | Apache-2.0/MIT | The Rust Project Developers |
| rustc-hash | 2.1.3 | Apache-2.0 OR MIT | The Rust Project Developers |
| rustc_version | 0.4.1 | MIT OR Apache-2.0 | - |
| rustix | 0.38.44 | Apache-2.0 WITH LLVM-exception OR Apache-2.0 OR MIT | Dan Gohman, Jakub Konka |
| rustix | 1.1.5 | Apache-2.0 WITH LLVM-exception OR Apache-2.0 OR MIT | Dan Gohman, Jakub Konka |
| rustversion | 1.0.23 | MIT OR Apache-2.0 | David Tolnay |
| same-file | 1.0.6 | Unlicense/MIT | Andrew Gallant |
| scoped-tls | 1.0.1 | MIT/Apache-2.0 | Alex Crichton |
| scoped-tls-hkt | 0.1.5 | MIT/Apache-2.0 | Diggory Blake |
| scopeguard | 1.2.0 | MIT OR Apache-2.0 | bluss |
| sctk-adwaita | 0.10.1 | MIT | Poly |
| semver | 1.0.28 | MIT OR Apache-2.0 | David Tolnay |
| serde | 1.0.229 | MIT OR Apache-2.0 | Erick Tryzelaar, David Tolnay |
| serde_core | 1.0.229 | MIT OR Apache-2.0 | Erick Tryzelaar, David Tolnay |
| serde_derive | 1.0.229 | MIT OR Apache-2.0 | Erick Tryzelaar, David Tolnay |
| serde_json | 1.0.151 | MIT OR Apache-2.0 | Erick Tryzelaar, David Tolnay |
| serde_repr | 0.1.21 | MIT OR Apache-2.0 | David Tolnay |
| serde_spanned | 1.1.1 | MIT OR Apache-2.0 | - |
| shlex | 1.3.0 | MIT OR Apache-2.0 | comex, Fenhl, Adrian Taylor, Alex Touchet, Daniel Parks, Garrett Berg |
| shlex | 2.0.1 | MIT OR Apache-2.0 | comex, Fenhl, Adrian Taylor, Alex Touchet, Daniel Parks, Garrett Berg |
| signal-hook-registry | 1.4.8 | MIT OR Apache-2.0 | Michal 'vorner' Vaner, Masaki Hara |
| simd-adler32 | 0.3.10 | MIT | Marvin Countryman |
| simd_cesu8 | 1.2.0 | Apache-2.0 OR MIT | Sean C. Roach |
| simdutf8 | 0.1.5 | MIT OR Apache-2.0 | Hans Kratz |
| simplecss | 0.2.2 | Apache-2.0 OR MIT | - |
| siphasher | 1.0.4 | MIT OR Apache-2.0 | Frank Denis |
| skia-bindings | 0.153.3 | MIT | LongYinan, Armin Sander |
| skia-safe | 0.153.3 | MIT | Armin Sander |
| skrifa | 0.44.0 | MIT OR Apache-2.0 | - |
| slab | 0.4.12 | MIT | Carl Lerche |
| slint | 1.18.1 | GPL-3.0-only OR LicenseRef-Slint-Royalty-free-2.0 OR LicenseRef-Slint-Software-3.0 | Slint Developers |
| slint-build | 1.18.1 | GPL-3.0-only OR LicenseRef-Slint-Royalty-free-2.0 OR LicenseRef-Slint-Software-3.0 | Slint Developers |
| slint-macros | 1.18.1 | GPL-3.0-only OR LicenseRef-Slint-Royalty-free-2.0 OR LicenseRef-Slint-Software-3.0 | Slint Developers |
| slotmap | 1.1.1 | Zlib | Orson Peters |
| smallvec | 1.16.2 | MIT OR Apache-2.0 | The Servo Project Developers |
| smithay-client-toolkit | 0.19.2 | MIT | Elinor Berger, i509VCB, Ashley Wulber |
| smol_str | 0.2.2 | MIT OR Apache-2.0 | Aleksey Kladov |
| smol_str | 0.3.6 | MIT OR Apache-2.0 | Aleksey Kladov, Lukas Wirth |
| snafu | 0.8.9 | MIT OR Apache-2.0 | Jake Goulding |
| snafu-derive | 0.8.9 | MIT OR Apache-2.0 | Jake Goulding |
| softbuffer | 0.4.8 | MIT OR Apache-2.0 | - |
| spin_on | 0.1.1 | Apache-2.0 OR MIT | paulkernfeld |
| spirv | 0.4.0+sdk-1.4.341.0 | Apache-2.0 | Lei Zhang |
| stable_deref_trait | 1.2.1 | MIT OR Apache-2.0 | Robert Grosse |
| static_assertions | 1.1.0 | MIT OR Apache-2.0 | Nikolai Vazquez |
| strict-num | 0.1.1 | MIT | Yevhenii Reizner |
| strum | 0.28.0 | MIT | Peter Glotfelty |
| strum_macros | 0.28.0 | MIT | Peter Glotfelty |
| svgtypes | 0.16.1 | Apache-2.0 OR MIT | - |
| swash | 0.2.10 | Apache-2.0 OR MIT | Chad Brokaw |
| syn | 2.0.119 | MIT OR Apache-2.0 | David Tolnay |
| syn | 3.0.6 | MIT OR Apache-2.0 | David Tolnay |
| synstructure | 0.14.0 | MIT | Nika Layzell |
| sys-locale | 0.3.2 | MIT OR Apache-2.0 | 1Password |
| taffy | 0.10.1 | MIT | Alice Cecile, Johnathan Kelley, Nico Burns |
| tar | 0.4.46 | MIT OR Apache-2.0 | Alex Crichton |
| tempfile | 3.27.0 | MIT OR Apache-2.0 | Steven Allen, The Rust Project Developers, Ashley Mannix, Jason White |
| text-size | 1.1.1 | MIT OR Apache-2.0 | Aleksey Kladov, Christopher Durham (CAD97) |
| thiserror | 1.0.69 | MIT OR Apache-2.0 | David Tolnay |
| thiserror | 2.0.21 | MIT OR Apache-2.0 | David Tolnay |
| thiserror-impl | 1.0.69 | MIT OR Apache-2.0 | David Tolnay |
| thiserror-impl | 2.0.21 | MIT OR Apache-2.0 | David Tolnay |
| tiny-skia | 0.11.4 | BSD-3-Clause | Yevhenii Reizner |
| tiny-skia | 0.12.0 | BSD-3-Clause | Yevhenii Reizner |
| tiny-skia-path | 0.11.4 | BSD-3-Clause | Yevhenii Reizner |
| tiny-skia-path | 0.12.0 | BSD-3-Clause | Yevhenii Reizner |
| tiny-xlib | 0.2.5 | MIT OR Apache-2.0 OR Zlib | John Nunley |
| tinystr | 0.8.4 | Unicode-3.0 | The ICU4X Project Developers |
| tinyvec | 1.13.3 | Zlib OR Apache-2.0 OR MIT | Lokathor |
| tokio | 1.53.1 | MIT | Tokio Contributors |
| toml | 1.1.6+spec-1.1.0 | MIT OR Apache-2.0 | - |
| toml_datetime | 1.1.1+spec-1.1.0 | MIT OR Apache-2.0 | - |
| toml_edit | 0.25.15+spec-1.1.0 | MIT OR Apache-2.0 | - |
| toml_parser | 1.1.3+spec-1.1.0 | MIT OR Apache-2.0 | - |
| toml_writer | 1.1.2+spec-1.1.0 | MIT OR Apache-2.0 | - |
| tracing | 0.1.44 | MIT | Eliza Weisman, Tokio Contributors |
| tracing-attributes | 0.1.31 | MIT | Tokio Contributors, Eliza Weisman, David Barsky |
| tracing-core | 0.1.36 | MIT | Tokio Contributors |
| tree_magic_mini | 3.2.2 | MIT | Matt Brubeck, Allison Hancock |
| ttf-parser | 0.25.1 | MIT OR Apache-2.0 | Caleb Maclennan, Laurenz Stampfl, Yevhenii Reizner, خالد حسني (Khaled Hosny) |
| typed-index-collections | 3.5.0 | MIT OR Apache-2.0 | Andrey Zheleznov |
| udev | 0.9.3 | MIT | David Cuddeback, Victoria Brekenfeld |
| uds_windows | 1.2.1 | MIT | Azure IoT Edge Devs, Harald Hoyer |
| unicase | 2.9.0 | MIT OR Apache-2.0 | Sean McArthur |
| unicode-bidi | 0.3.18 | MIT OR Apache-2.0 | The Servo Project Developers |
| unicode-ident | 1.0.26 | (MIT OR Apache-2.0) AND Unicode-3.0 | David Tolnay |
| unicode-linebreak | 0.1.5 | Apache-2.0 | Axel Forsman |
| unicode-script | 0.5.8 | MIT OR Apache-2.0 | Manish Goregaokar |
| unicode-segmentation | 1.13.3 | MIT OR Apache-2.0 | kwantam, Manish Goregaokar |
| unicode-vo | 0.1.0 | MIT/Apache-2.0 | Evgeniy Reizner |
| unicode-width | 0.2.2 | MIT OR Apache-2.0 | kwantam, Manish Goregaokar |
| unicode-xid | 0.2.6 | MIT OR Apache-2.0 | erick.tryzelaar, kwantam, Manish Goregaokar |
| unty | 0.0.4 | MIT OR Apache-2.0 | Victor Koenders |
| url | 2.5.8 | MIT OR Apache-2.0 | The rust-url developers |
| usvg | 0.48.1 | Apache-2.0 OR MIT | - |
| utf8_iter | 1.0.4 | Apache-2.0 OR MIT | Henri Sivonen |
| uuid | 1.26.1 | Apache-2.0 OR MIT | Ashley Mannix, Dylan DPC, Hunar Roop Kahlon |
| version_check | 0.9.5 | MIT/Apache-2.0 | Sergio Benitez |
| vtable | 0.5.0 | MIT OR Apache-2.0 | Slint Developers |
| vtable-macro | 0.5.0 | MIT OR Apache-2.0 | Slint Developers |
| walkdir | 2.5.0 | Unlicense/MIT | Andrew Gallant |
| wasip2 | 1.0.4+wasi-0.2.12 | Apache-2.0 WITH LLVM-exception OR Apache-2.0 OR MIT | - |
| wasm-bindgen | 0.2.129 | MIT OR Apache-2.0 | The wasm-bindgen Developers |
| wasm-bindgen-futures | 0.4.79 | MIT OR Apache-2.0 | The wasm-bindgen Developers |
| wasm-bindgen-macro | 0.2.129 | MIT OR Apache-2.0 | The wasm-bindgen Developers |
| wasm-bindgen-macro-support | 0.2.129 | MIT OR Apache-2.0 | The wasm-bindgen Developers |
| wasm-bindgen-shared | 0.2.129 | MIT OR Apache-2.0 | The wasm-bindgen Developers |
| wayland-backend | 0.3.17 | MIT | Elinor Berger |
| wayland-client | 0.31.15 | MIT | Elinor Berger |
| wayland-csd-frame | 0.3.0 | MIT | Kirill Chibisov |
| wayland-cursor | 0.31.14 | MIT | Elinor Berger |
| wayland-protocols | 0.32.13 | MIT | Elinor Berger |
| wayland-protocols-plasma | 0.3.12 | MIT | Elinor Berger |
| wayland-protocols-wlr | 0.3.12 | MIT | Elinor Berger |
| wayland-scanner | 0.31.11 | MIT | Elinor Berger |
| wayland-sys | 0.31.11 | MIT | Elinor Berger |
| web-sys | 0.3.106 | MIT OR Apache-2.0 | The wasm-bindgen Developers |
| web-time | 1.1.0 | MIT OR Apache-2.0 | - |
| webbrowser | 1.2.4 | MIT OR Apache-2.0 | Amod Malviya @amodm |
| weezl | 0.1.12 | MIT OR Apache-2.0 | The image-rs Developers |
| wgpu | 30.0.1 | MIT OR Apache-2.0 | gfx-rs developers |
| wgpu-core | 30.0.1 | MIT OR Apache-2.0 | gfx-rs developers |
| wgpu-core-deps-apple | 30.0.1 | MIT OR Apache-2.0 | gfx-rs developers |
| wgpu-core-deps-windows-linux-android | 30.0.1 | MIT OR Apache-2.0 | gfx-rs developers |
| wgpu-hal | 30.0.1 | MIT OR Apache-2.0 | gfx-rs developers |
| wgpu-naga-bridge | 30.0.1 | MIT OR Apache-2.0 | gfx-rs developers |
| wgpu-types | 30.0.1 | MIT OR Apache-2.0 | gfx-rs developers |
| winapi-util | 0.1.11 | Unlicense OR MIT | Andrew Gallant |
| windows | 0.62.2 | MIT OR Apache-2.0 | - |
| windows-collections | 0.3.2 | MIT OR Apache-2.0 | - |
| windows-core | 0.62.2 | MIT OR Apache-2.0 | - |
| windows-future | 0.3.2 | MIT OR Apache-2.0 | - |
| windows-implement | 0.60.2 | MIT OR Apache-2.0 | - |
| windows-interface | 0.59.3 | MIT OR Apache-2.0 | - |
| windows-link | 0.2.1 | MIT OR Apache-2.0 | - |
| windows-numerics | 0.3.1 | MIT OR Apache-2.0 | - |
| windows-result | 0.4.1 | MIT OR Apache-2.0 | - |
| windows-strings | 0.5.1 | MIT OR Apache-2.0 | - |
| windows-sys | 0.48.0 | MIT OR Apache-2.0 | Microsoft |
| windows-sys | 0.52.0 | MIT OR Apache-2.0 | Microsoft |
| windows-sys | 0.59.0 | MIT OR Apache-2.0 | Microsoft |
| windows-sys | 0.60.2 | MIT OR Apache-2.0 | Microsoft |
| windows-sys | 0.61.2 | MIT OR Apache-2.0 | - |
| windows-targets | 0.48.5 | MIT OR Apache-2.0 | Microsoft |
| windows-targets | 0.52.6 | MIT OR Apache-2.0 | Microsoft |
| windows-targets | 0.53.5 | MIT OR Apache-2.0 | - |
| windows-threading | 0.2.1 | MIT OR Apache-2.0 | - |
| windows_aarch64_gnullvm | 0.48.5 | MIT OR Apache-2.0 | Microsoft |
| windows_aarch64_gnullvm | 0.52.6 | MIT OR Apache-2.0 | Microsoft |
| windows_aarch64_gnullvm | 0.53.1 | MIT OR Apache-2.0 | - |
| windows_aarch64_msvc | 0.48.5 | MIT OR Apache-2.0 | Microsoft |
| windows_aarch64_msvc | 0.52.6 | MIT OR Apache-2.0 | Microsoft |
| windows_aarch64_msvc | 0.53.1 | MIT OR Apache-2.0 | - |
| windows_i686_gnu | 0.48.5 | MIT OR Apache-2.0 | Microsoft |
| windows_i686_gnu | 0.52.6 | MIT OR Apache-2.0 | Microsoft |
| windows_i686_gnu | 0.53.1 | MIT OR Apache-2.0 | - |
| windows_i686_gnullvm | 0.52.6 | MIT OR Apache-2.0 | Microsoft |
| windows_i686_gnullvm | 0.53.1 | MIT OR Apache-2.0 | - |
| windows_i686_msvc | 0.48.5 | MIT OR Apache-2.0 | Microsoft |
| windows_i686_msvc | 0.52.6 | MIT OR Apache-2.0 | Microsoft |
| windows_i686_msvc | 0.53.1 | MIT OR Apache-2.0 | - |
| windows_x86_64_gnu | 0.48.5 | MIT OR Apache-2.0 | Microsoft |
| windows_x86_64_gnu | 0.52.6 | MIT OR Apache-2.0 | Microsoft |
| windows_x86_64_gnu | 0.53.1 | MIT OR Apache-2.0 | - |
| windows_x86_64_gnullvm | 0.48.5 | MIT OR Apache-2.0 | Microsoft |
| windows_x86_64_gnullvm | 0.52.6 | MIT OR Apache-2.0 | Microsoft |
| windows_x86_64_gnullvm | 0.53.1 | MIT OR Apache-2.0 | - |
| windows_x86_64_msvc | 0.48.5 | MIT OR Apache-2.0 | Microsoft |
| windows_x86_64_msvc | 0.52.6 | MIT OR Apache-2.0 | Microsoft |
| windows_x86_64_msvc | 0.53.1 | MIT OR Apache-2.0 | - |
| winit | 0.30.13 | Apache-2.0 | The winit contributors, Pierre Krieger |
| winnow | 1.0.4 | MIT | - |
| wit-bindgen | 0.57.1 | Apache-2.0 WITH LLVM-exception OR Apache-2.0 OR MIT | Alex Crichton |
| wl-clipboard-rs | 0.9.4 | MIT OR Apache-2.0 | Ivan Molodetskikh |
| write-fonts | 0.50.0 | MIT OR Apache-2.0 | - |
| writeable | 0.6.4 | Unicode-3.0 | The ICU4X Project Developers |
| x11-dl | 2.21.0 | MIT | daggerbot, Erle Pereira, AltF02 |
| x11rb | 0.13.2 | MIT OR Apache-2.0 | Uli Schlachter, Eduardo Sánchez Muñoz, notgull |
| x11rb-protocol | 0.13.2 | MIT OR Apache-2.0 | Uli Schlachter, Eduardo Sánchez Muñoz, notgull |
| xattr | 1.6.1 | MIT OR Apache-2.0 | Steven Allen |
| xcursor | 0.3.11 | MIT | Samuele Esposito |
| xkbcommon | 0.9.0 | MIT | Remi THEBAULT |
| xkbcommon-dl | 0.4.2 | MIT | Francesca Frangipane |
| xkeysym | 0.2.1 | MIT OR Apache-2.0 OR Zlib | John Nunley |
| xml-rs | 0.8.29 | MIT | Vladimir Matveev |
| xmlwriter | 0.1.0 | MIT | Evgeniy Reizner |
| yazi | 0.2.1 | Apache-2.0 OR MIT | Chad Brokaw |
| yeslogic-fontconfig-sys | 6.0.1 | MIT | Austin Bonander, The Servo Project Developers, YesLogic Pty. Ltd. |
| yoke | 0.8.3 | Unicode-3.0 | Manish Goregaokar |
| yoke-derive | 0.8.4 | Unicode-3.0 | Manish Goregaokar |
| zbus | 5.19.0 | MIT | Zeeshan Ali Khan |
| zbus_macros | 5.19.0 | MIT | Marc-André Lureau, Zeeshan Ali Khan |
| zbus_names | 4.3.4 | MIT | Zeeshan Ali Khan |
| zcheapstr | 1.1.0 | MIT | Zeeshan Ali Khan |
| zeno | 0.3.3 | Apache-2.0 OR MIT | Chad Brokaw |
| zerocopy | 0.8.59 | BSD-2-Clause OR Apache-2.0 OR MIT | - |
| zerocopy-derive | 0.8.59 | BSD-2-Clause OR Apache-2.0 OR MIT | - |
| zerofrom | 0.1.8 | Unicode-3.0 | The ICU4X Project Developers |
| zerofrom-derive | 0.1.8 | Unicode-3.0 | Manish Goregaokar |
| zerotrie | 0.2.5 | Unicode-3.0 | The ICU4X Project Developers |
| zerovec | 0.11.8 | Unicode-3.0 | The ICU4X Project Developers |
| zerovec-derive | 0.11.6 | Unicode-3.0 | Manish Goregaokar |
| zlib-rs | 0.6.8 | Zlib | - |
| zmij | 1.0.23 | MIT | David Tolnay |
| zune-core | 0.5.3 | MIT OR Apache-2.0 OR Zlib | - |
| zune-jpeg | 0.5.15 | MIT OR Apache-2.0 OR Zlib | caleb |
| zvariant | 5.15.0 | MIT | Zeeshan Ali Khan |
| zvariant_derive | 5.15.0 | MIT | Zeeshan Ali Khan |
| zvariant_utils | 4.2.0 | MIT | Zeeshan Ali Khan, turbocooler |

Licence choices where several are offered: MIT is chosen for `MIT OR Apache-2.0 [OR …]` and `… OR LGPL-2.1-or-later`; Apache-2.0 with LLVM exception terms are not needed; Unicode-3.0 and Zlib terms apply to the crates that carry them.
