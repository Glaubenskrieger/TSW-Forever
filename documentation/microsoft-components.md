# Microsoft DirectX components — EU/German assessment and consent

Reviewed 2026-11-03. This is a source-based engineering/legal risk assessment, not binding legal advice or a rights-holder authorisation.

**Decision: CLARIFIED WITH ADJUSTMENTS for the private technical rc.4 (Variant B).** The user-side download/extraction path has a reasonably supportable permission basis, provided the original terms are available and explicitly accepted before new component provisioning. No concrete serious infringement or unacceptable risk was established. Remaining interpretation questions are retained for the owner's separate public-release review; they are not declared legally adjudicated. A private repository is not itself an exemption from copyright/licence duties.

## rc.5 setup correction — 2026-11-04

The published rc.4 remains unchanged and uses the inline package-term flow described below. The owner has approved online access to the original terms for rc.5: both independent checkboxes are available before **INSTALL THE GAME**, which stays disabled until both are checked. No Microsoft package is downloaded just to enable the checkbox. Opening a link does not accept it. The existing package/text verification and package-bound local acceptance record remain in the provisioning path; neither an unknown package nor changed original resources can inherit acceptance.

The online copies were compared with both original agreements from the pinned package. After normalising only whitespace, typographic quotation marks and bullet glyphs, the complete SDK text matches, and the complete runtime text is present. These are explicitly labelled **online copies, not Microsoft-hosted pages**:

- SDK: [original text, pinned apitrace source revision](https://github.com/apitrace/dxsdk/blob/d964b66467aaa734edbc24326da8119f5f063dd3/Documentation/License%20Agreements/DirectX%20SDK%20EULA.txt).
- Runtime: [Microsoft DirectX End User Runtime section, Steam-hosted copy](https://store.steampowered.com/eula/17300_eula_1). The page also contains unrelated product conditions; those are not our agreement and are not accepted through the Microsoft checkbox.

Only these two fixed HTTPS destinations can be opened, using the desktop OpenURI portal without added host filesystem permissions. An error submitting the portal request is reported; no consent is automatically supplied. The link content/package association must be rechecked when changing the package or the displayed terms. No replacement of the tested download/extraction logic or execution of Microsoft's installer is introduced. The prior EU/German residual interpretation question remains; online access is a presentation change, not an expansion of rights or a public-release clearance.

## 1. Actual facts and primary package evidence

This is a non-commercial, independently written GPL community launcher for private use of lawfully accessed original The Secret World (2012). The launcher is not a game server/account service. Its artifacts contain neither Funcom game files nor Microsoft executables/DLLs. The GPL launcher starts external Wine/game processes; it does not incorporate Microsoft's DLL object code into its Rust/Slint binary or purport to relicense it under GPL.

Official product page: https://www.microsoft.com/en-us/download/details.aspx?id=8109

Exact user download: https://download.microsoft.com/download/8/4/A/84A35BF1-DAFE-4AE8-82AF-AD2AE20B6B14/directx_Jun2010_redist.exe

SHA256: `053f76dcbb28802e23341b6a787e3b0791c0fa5c8d4d011b1044172dbf89c73b`; 100,275,120 bytes. Existing private evidence was reused, not downloaded again. Package/setup resources were inspected as data; no Microsoft executable was run. Hashes and method are retained privately in `reports/rc4/microsoft-evidence.json`.

The standard path extracts only `D3DCompiler_43.dll`, `d3dx9_43.dll`, `d3dx11_43.dll` from their three x86 June-2010 cabinets, verifies the outer SHA256 and each DLL's expected size/MD5, and places unmodified DLLs in the user's managed Wine prefix. Compiler 47 is not downloaded or distributed. Cabinet decompression/resource reading does not decompile the machine instructions into source code, and the implementation removes no DRM, activation, encryption or account gate. It is not a means solely intended to defeat program protection.

### Two separate agreements in the exact package

- **SDK agreement**, section 1(a): installation/use on the user's devices; section 1(b): included programs have their own terms. Section 2(b)(ii)-(iii) regulates redistribution, with additional obligations and a Windows/Xbox/Windows-Mobile platform restriction. Section 3 includes technical-limitations/reverse-engineering restrictions subject to applicable law. Section 9(b) refers acquisitions outside the US to the law of the acquisition country.
- **Microsoft DirectX End User Runtime agreement**, embedded in `dsetup32.dll`: section 1 grants installation/use on the user's devices. Section 2 restricts copying/publication/transfer and circumvention of technical limitations, with the stated statutory qualification for reverse engineering. Unlike the SDK redistribution clause, it contains no express Windows-only device condition. Sections 8(b) and 9 address applicable local law and preserved legal rights.

The first English runtime text starts at offset 483592 in the inspected DLL. Raw SDK text SHA256: `500b2c1e82c511e575fa443e3a686c7098097974e91f06a9f443e4c8f1bf10a4`; raw runtime resource SHA256: `49385cb31461a0448f02053b17d73c42bbbcd27bf38d697a907eba4442d5f479`. The resources are checked in addition to the package before display. No original Microsoft licence file is embedded in our release: full text is obtained locally from the user's verified package.

## 2. Separate legal acts

| Act | Assessment | Boundary |
|---|---|---|
| Our redistribution of Microsoft DLLs | Not authorised by this project decision and not implemented. SDK redistribution conditions would require their own analysis. | Do not put DLLs/setup executables in our artifacts, source, server or public staging. |
| User downloads Microsoft's freely offered official package | Reasonably supported by Microsoft's offer and the accompanying personal installation/use grant, subject to their applicable conditions. | Free download is not public domain or permission to mirror/redistribute. |
| User runs original setup | Clearly the vendor-provided technical route; included terms still apply. | Wine support, extra system actions and installer behaviour are not established by the licence alone. |
| User extracts unmodified DLLs locally | Reasonably construed as local installation/reproduction for the granted runtime use, not our redistribution. | No court decision on this exact method was located; do not extend this to changing binaries, removing protection or sharing files. |
| Loading those DLLs in Wine for an entitled game | The DLLs still perform their intended compiler/D3DX functions. The inspected runtime grant has no express Windows-only condition. | Supported-Windows lists are technical support statements; they are not automatically either an additional licence ban or Linux endorsement. |
| A user-started launcher automates the local steps | Automation does not, by itself, turn a user-side copy into our distribution or expand the user's rights. | Transparent separate consent, exact package pinning, no credential/activation bypass and no hidden background acquisition remain required. |

The SDK's non-Windows **redistribution** clause is not simply an end-user Wine prohibition. Conversely, no generic interoperability slogan overrides a genuine applicable restriction. There is no claim that absence of an express prohibition alone establishes permission: the positive personal installation/use grant and the exact unmodified/local behaviour are central.

## 3. EU and German software rules

Primary EU source: [Directive 2009/24/EC, OJ L 111, 5 May 2009, pp. 16–22](https://eur-lex.europa.eu/legal-content/EN/ALL/?uri=CELEX:32009L0024). The relevant German provisions are in [UrhG §§69a–69g](https://www.gesetze-im-internet.de/urhg/BJNR012730965.html).

### Protected expression and reproduction

Article 1(2) / §69a(2) distinguish protected expression from ideas/principles underlying interfaces. Article 4 / §69c still reserve reproduction, adaptation and distribution, including copies needed for loading/storage. Independence, open-source licensing and a compatibility purpose do not permit copying Microsoft's implementation into our source. Our model instead uses separately obtained, unmodified proprietary components on the user's device.

### Necessary lawful use: Article 5(1), §69d(1)

These provisions concern acts necessary for intended use, including correction of errors, by a lawful acquirer/person entitled to use the copy. They are not a right to acquire software unlawfully. Their wording recognises specific contractual provisions: it is wrong to say all usage restrictions are automatically void. Here the lawful-use basis is strengthened by Microsoft's own device-use grant and official download. Whether a particular platform/method limitation would validly narrow intended use remains interpretation-sensitive. We do not assert that every unsupported-system issue is a statutory error-correction case.

### Observation/testing and interoperability: Articles 5(3), 6 and 8; §§69d(3), 69e, 69g(2)

Lawful observation/testing may determine underlying ideas/principles during acts the user is entitled to perform. Decompilation to obtain interoperability information is a different, narrowly conditioned exception: an entitled person (or authorised person acting for them), indispensable acts, information not otherwise readily available, and restriction to necessary parts. Resulting information cannot be reused for unrelated purposes, unnecessary disclosure, substantially similar protected expression or infringement; normal exploitation and legitimate interests remain protected. Article 8's second paragraph / §69g(2) invalidate contrary terms within the specified mandatory exceptions. They do not invalidate all EULA provisions or erase separate acquisition/copying requirements.

**Application here:** ordinary cabinet extraction and loading a runtime are not a decompilation project to discover unavailable interoperability information. We therefore do not rely on §69e as blanket permission for download or installation. The interoperability framework supports independently created compatibility software and limits certain contractual restrictions; it does not replace the relevant personal-use grant for this specific path.

### Relevant judgments, not a decision on our exact facts

- [CJEU, 2 May 2012, C-406/10, SAS Institute, ECLI:EU:C:2012:259](https://eur-lex.europa.eu/legal-content/EN/TXT/?uri=CELEX:62010CJ0406), especially paragraphs 39–46 and 49–61: functionality is not protected program expression as such; lawful observation/testing is protected within its conditions. Copying protected source/object-code expression remains different. This is useful for independent compatibility work, not a DLL redistribution licence.
- [CJEU, 6 October 2021, C-13/20, Top System, ECLI:EU:C:2021:811](https://eur-lex.europa.eu/legal-content/EN/ALL/?uri=CELEX:62020CJ0013), paragraphs 58–69 and operative part: necessary error-correction decompilation is not limited to Article 6; parties cannot prohibit every possibility of correction but can organise its exercise. Necessity, lawful acquisition, purpose and contract conditions remain relevant. We are not treating Wine installation as proven error correction under this judgment.

These judgments apply the predecessor Directive 91/250; the corresponding rules are codified in Directive 2009/24. Neither adjudicates the June-2010 extraction method under Wine. No such directly controlling ruling was located.

## 4. Private/non-commercial purpose and applicable law

Non-commercial development and private gameplay support the limited factual purpose and avoid a commercial redistribution/hosting model; they are **not general copyright exceptions**. A free GPL launcher grants no rights to another party's binaries. Software has the specific §§69c–69g regime; do not assume a general private-copy privilege authorises every software copy or source.

For contracts, [Rome I Regulation 593/2008](https://eur-lex.europa.eu/legal-content/en/TXT/?uri=CELEX:32008R0593), Articles 3, 4 and 6, distinguishes choice/default law and qualifying consumer contracts. If a professional directs relevant activity to a consumer's residence state, a choice cannot remove its otherwise applicable mandatory protection. The inspected Microsoft agreements themselves refer non-US acquisitions to the acquisition country's law. This supports examining German law for German private users, not automatically applying Washington law worldwide. Exact contracting entity, residence, terms incorporation and facts still matter.

For non-contractual IP claims, [Rome II Regulation 864/2007, Article 8](https://eur-lex.europa.eu/legal-content/en/TXT/?uri=CELEX:32007R0864) points to the country for which protection is claimed; its rule cannot be displaced by a choice under Article 14. A contract-law clause therefore does not alone decide German copyright claims. The directive is implemented through national law; not every contractual question is answered directly by its text.

German [BGB §305(2)](https://www.gesetze-im-internet.de/bgb/__305.html) requires appropriate notice/opportunity to know incorporated standard terms and agreement; [§307](https://www.gesetze-im-internet.de/bgb/__307.html) controls unfair/nontransparent terms where applicable. An in-UI acknowledgement improves evidence and transparency but does not give our project authority to contract for Microsoft or waive mandatory rights. The exact formation/effect of the user's Microsoft relationship remains a legal question; we do not rely on supposedly ineffective terms to manufacture rights.

Public distribution of our own launcher separately requires its GPL/third-party compliance and respect for trademarks and other law. German/EU focus does not automatically exclude claims in other jurisdictions after worldwide publication. The private technical release is not an assurance for every country or a public legal clearance.

## 5. Technical variants and chosen safeguards

| Variant | Legal support / uncertainty | Compatibility / UX | Risk and test effort | Decision |
|---|---|---|---|---|
| A: unchanged flow | Personal-use basis exists, but download-only consent poorly distinguishes applicable Microsoft terms. | Working path unchanged. | Low code risk; avoidable notice/evidence weakness. | Not preferred. |
| B: same path, explicit original-term review/consent | Separates personal acquisition from our distribution and preserves applicable terms/mandatory rights. Wine/method interpretation remains disclosed. | Same pinned source, exact DLLs and Wine setup; integrated in existing setup view. | Small focused implementation; refusal/resume/cache/UI regressions and actual-package static extraction checked. | **Chosen for private rc.4.** |
| C: run Microsoft setup | Vendor mechanism has a stronger technical provenance for installation, but is not automatic proof of licence legality on Wine. | Could add components/prompts, perform compatibility checks or change the functioning prefix. | Requires new dynamic execution permission, isolated Wine trials and broader regression effort. | Not justified by the established risk; not run. |

Download consent remains separate. Full original SDK/runtime terms are shown in a scrollable area **inside the existing setup view** before Microsoft acceptance. Its own checkbox starts unchecked and cannot be enabled before the verified text is available; no extra installation wizard/modal is introduced. Continue requires the explicit Microsoft choice. Decline stops new provisioning and automatic continuation without deleting the prefix or installed game. The verified download is retained for retry. Exact package and both reviewed text hashes bind the local acknowledgement; a different package/terms cannot inherit it.

Previously installed game profiles are not rewritten or forced to reconfirm at each start. Reusing cached DLLs for a new provisioning step requires the current acknowledgement; unacknowledged old cache data alone is insufficient. Existing installations are not retroactively declared legally licensed by our UI. The CLI presents the terms and requires separate `--accept-microsoft-license` in addition to download consent. Acknowledgement/text files stay in the user's private component cache, never in release artifacts. Any derived permissions remain limited to the inspected acts and a lawfully used game.

## 6. Decision, remaining question and Phase 3 boundary

**Private engineering gate: CLARIFIED WITH ADJUSTMENTS**, once these consent safeguards and the full technical/security checks pass. The owner's explicit updated decision permits a private technical candidate with transparent, reasonably defensible residual uncertainty. The previous automatic block merely for absence of an express Wine/extraction endorsement was too broad; it is superseded, not silently erased (original assessment retained privately).

**Remaining interpretation question:** would a court construe the personal device-use grant, the generic technical-limitations clause and any properly incorporated platform/method condition to cover the user's unmodified cabinet extraction and Wine loading in this exact setting? No affirmative serious prohibition was established; neither a definitive court ruling nor individual Microsoft assurance is claimed. Obtaining a qualified opinion or clarification remains an option for the owner before public distribution, especially if the factual scope changes. A request has not been sent.

**Actual prohibited/unapproved changes:** distributing Microsoft's binaries under our GPL, mirroring them, modifying notices, evading activation/DRM/account checks, or treating interoperability/non-commercial status as unlimited permission. None is implemented or approved.

At the time of this private rc.4 assessment, public Phase 3 was unapproved. The owner has since published public `0.2.0`; that decision does not turn the residual interpretation question into a settled legal conclusion. Re-evaluate it if the package, terms, installation method or distribution scope changes. A serious concrete contrary finding requires review; ordinary disclosed uncertainty is not relabelled as certainty.

## 7. Branding

The project uses its own vector/generated icons, no Funcom logo payload. `assets/branding/Dramatisches Fraktions-Emblem mit Ankh.png` lacks established provenance/rights and stays private and excluded by source selection/public-snapshot checks. Do not delete the private file or move it into public source/marketing. Project descriptions explicitly state unofficial status and no Funcom affiliation; GPL grants no game/trademark rights.

## 8. Windows (version 0.3.0)

The assessment above concerns the Linux flow, in which the launcher downloads and extracts Microsoft components for the Wine prefix. The native Windows build does not do that. It only checks whether the 32-bit Direct3D libraries of the DirectX End-User Runtimes (June 2010) exist (`d3dx9_43.dll`, `d3dx11_43.dll`, `d3dcompiler_43.dll`). If something is missing, the user may click **Install DirectX components …**: the launcher then downloads Microsoft's own web installer `dxwebsetup.exe` from `download.microsoft.com`, compares it with a pinned SHA-256 value, checks Microsoft's Authenticode signature and starts it through the Windows shell. Microsoft's installer shows and records its own terms; the launcher passes no switch that accepts or hides them, extracts nothing and distributes no Microsoft file. Nothing is downloaded without that click.
