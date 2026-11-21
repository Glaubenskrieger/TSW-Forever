/* Site settings. The site is static and has no backend: the download buttons ask GitHub once for the newest PUBLISHED release (js/download.js), so no version number and no file name
   of a release is written into the pages. The installation files are assets of that release, never files of this site. */
window.TSW_SITE = {
  /* The project's repository. The release files are looked for in this repository only. */
  repo: "Glaubenskrieger/TSW-Forever",
  storagePrefix: "tsw-forever.",
  themes: ["templars", "illuminati", "dragon"],
  defaultTheme: "templars",
  languages: ["en", "de", "fr", "es", "pt-BR", "pl"],
  defaultLanguage: "en"
};
