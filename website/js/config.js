/* Site settings.
   updateSourceLive decides whether the page shows the signed Flatpak update source as available:
     "auto" (default)  js/source.js asks THIS site whether the source exists: repo/summary, tsw-forever.flatpakrepo and the .flatpakref must all answer. A host name proves nothing
                       (a *.github.io page can exist without the repository), so it is never used for the decision. On the local preview they do not exist: "not active yet".
     true / false      force the one or the other text (for a preview or an emergency; no owner step is needed for the normal case). */
window.TSW_SITE = {
  updateSourceLive: "auto",
  sourceProbe: ["repo/summary", "tsw-forever.flatpakrepo", "org.tswlauncher.Launcher.flatpakref"],
  /* The public stable repository. The Windows download link is NOT written down here: js/download.js asks the GitHub API for the newest release and accepts only
     an installer asset of the exact shape tsw-forever-<MAJOR.MINOR.PATCH>-windows-setup.exe from this repository, so a future release needs no change on this site. */
  repo: "Glaubenskrieger/TSW-Forever",
  storagePrefix: "tsw-forever.",
  themes: ["templars", "illuminati", "dragon"],
  defaultTheme: "templars",
  languages: ["en", "de", "fr", "es", "pt-BR", "pl"],
  defaultLanguage: "en"
};
