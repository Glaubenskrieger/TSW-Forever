#!/usr/bin/env python3
"""Central masking of secrets for everything this project records or reports (one place, used by all tools).

Why: the original client writes its chat registration (nick, user, real name and a `password:` field; whether that value is a
session token or a permanent password is unknown, so it is treated as a permanent password) to its own log and to the console.
We cannot change the client. We can make sure that nothing WE record, copy into reports or print on purpose contains it.

Three layers, strongest first:
  1. WITHHOLD a whole line when it looks like it carries credentials, tokens, cookies or personal names (conservative on
     purpose: a harmless line may be withheld, a secret line must never pass). The placeholder says only THAT a line was withheld.
  2. MASK values inside lines that are kept: credentials in URLs (`scheme://user:pw@host`), `key=value`/`"key": "value"`/query
     parameters of secret-like keys, `Authorization:`/`Bearer` values, JSON web tokens, e-mail addresses, long opaque strings,
     the home directory. Spellings: upper/lower case, `pass_word`, `pass-word`, `p4ssw0rd`, percent-encoded separators.
  3. CONTINUATIONS: a withheld line that ends in `:`/`=`/`\\` or leaves a quote open also takes the following line with it
     (multi-line values), and a bare key line (`password:`) takes its value line.
What is kept deliberately: timestamps, module loads (`Loaded L"…\\d3dcompiler_47.dll" … native`), Wine/DXVK error text, shader
compile errors, D3DX/Cheetah errors, identifiers made of real words (`D3DXCreateTextureFromFileInMemoryEx`).

Limits (stated, not hidden): the filter works on text lines. A secret that is split over several lines in a way not covered, binary
output, or a secret without any recognisable key/context in a file we do not control can pass. The original client's own
ClientLog.txt is NOT rewritten by us and contains the chat line: it must be treated as private.
"""
import re

PLACEHOLDER = "<line withheld: may contain credentials or names>"

_PASS = r"p[a4@]ss[\s_.\-]*(?:w[o0]rd|wd|phrase|code)?"
# keys that make a line "secret-like" (word-ish match, any case)
SECRET_KEY = (
    rf"(?:{_PASS}|pwd|secret|tokens?|api[\s_.\-]*key|access[\s_.\-]*key|private[\s_.\-]*key|authori[sz]ation|bearer|set-cookie|cookie|"
    r"login[\s_.\-]*key|stat[\s_.\-]*index|ticket|credentials?|session[\s_.\-]*(?:id|key|token)?|oauth|jwt)"
)
# conservative whole-line triggers. Key words only count as WORDS (no letter directly before or after, a plural s is fine), so the
# game's own name (TheSecretWorldDX11.exe, secretworld.com) does not trigger them, while `client_secret`, `Secret:` and `Pass-Word` do.
SENSITIVE_LINE = re.compile(
    rf"(?<![A-Za-z])(?:{SECRET_KEY}|nick(?:name)?|account|logon|user[\s_.\-]?name|char(?:acter)?[\s_.\-]?name|e-?mail|steamid|registering)s?(?![A-Za-z])"
    r"|(?<![A-Za-z])(?:login|player|character)(?![A-Za-z])",
    re.I,
)
# IRC-style registration lines the chat server connection speaks: PASS/NICK/USER (with a trailing :real name)
IRC = re.compile(r"(?im)^\W*(?:PASS|NICK|USER|OPER)\s+\S+|\bUSER\s+\S+\s+\S+\s+\S+\s+:")

URL_CREDS = re.compile(r"(?i)\b([a-z][a-z0-9+.\-]*://)[^/\s:@]*:[^/\s@]*@")
URL_USERONLY = re.compile(r"(?i)\b([a-z][a-z0-9+.\-]*://)[^/\s:@]+@")
KEYVAL = re.compile(
    rf"(?i)((?:[\"']?)(?:{SECRET_KEY}|key|nick(?:name)?|user(?:name)?|real(?:name)?)(?:[\"']?)\s*(?:[:=]|%3d|%3a)\s*)(?:\"[^\"]*\"|'[^']*'|[^\s&;,\"']+)"
)
AUTH_HEADER = re.compile(r"(?i)\b(authorization\s*:\s*(?:basic|bearer|token)?\s*)\S+")
BEARER = re.compile(r"(?i)\b(bearer\s+)[A-Za-z0-9._\-~+/]{8,}=*")
JWT = re.compile(r"\beyJ[A-Za-z0-9_\-]{8,}\.[A-Za-z0-9_\-]{8,}\.[A-Za-z0-9_\-]*")
SECRET_FLAG = re.compile(r"(?i)(-{1,2}(?:loginkey|statindex|login[_-]?key|password|passwd|token)\s+)\S+")
EMAIL = re.compile(r"[A-Za-z0-9._%+\-]+@[A-Za-z0-9.\-]+\.[A-Za-z]{2,}")
OPAQUE = re.compile(r"(?<![A-Za-z0-9+/_\-])[A-Za-z0-9+/_\-]{16,}={0,2}(?![A-Za-z0-9+/_\-])")
# identifiers made of real words are not secrets (D3DXCreateTextureFromFileInMemoryEx, d3dcompiler_47.dll, …)
WORDY = re.compile(
    r"\b(?=[A-Za-z0-9_]{16,}\b)(?:[A-Za-z0-9_]*?[a-z]{3,}){3,}[A-Za-z0-9_]*"
    r"|\b(?:D3D|DXGI|IDXGI|ID3D|VKD3D|vkd3d)[A-Za-z0-9_]{4,}"
    r"|\bTheSecretWorld(?:DX11)?\b|\b(?:ClientPatcher|AwesomiumProcess)\b"   # the game's own program names (tests/logs name them)
    r"|\bVK_[A-Z0-9_]{4,}\b"                                                                 # Vulkan constants (VK_FORMAT_B8G8R8A8_UNORM)
    r"|\b(?:ab-)?V\d-\d{8}T\d{6}\b|\b\d{8}T\d{6}\b"                                     # run stamps of our own reports
    r"|(?<=\bat )[0-9A-Fa-f]{8,16}(?=:)|\b0x[0-9A-Fa-f]{6,16}\b"              # load addresses and pointers: hex, not secrets
)
_BARE_KEY = re.compile(rf"(?i)\b(?:{SECRET_KEY})\b[\"']?\s*[:=]\s*[\"']?\s*$")


def is_sensitive_line(line: str) -> bool:
    """True if the whole line must be withheld."""
    return bool(SENSITIVE_LINE.search(line) or IRC.search(line))


def _mask_values(line: str, home: str | None) -> str:
    keep: list[str] = []

    def park(m: re.Match) -> str:
        keep.append(m.group(0))
        return f"\x00{len(keep) - 1}\x00"

    s = WORDY.sub(park, line)
    s = URL_CREDS.sub(r"\1<masked>@", s)
    s = URL_USERONLY.sub(r"\1<masked>@", s)
    s = AUTH_HEADER.sub(r"\1<masked>", s)
    s = BEARER.sub(r"\1<masked>", s)
    s = JWT.sub("<masked-jwt>", s)
    s = SECRET_FLAG.sub(r"\1<masked>", s)
    s = KEYVAL.sub(lambda m: m.group(1) + "<masked>", s)
    s = EMAIL.sub("<masked-email>", s)
    s = OPAQUE.sub(lambda m: "<masked>" if re.search(r"\d", m.group(0)) and re.search(r"[A-Za-z]", m.group(0)) and "/" not in m.group(0) else m.group(0), s)
    if home and len(home) > 1:
        s = s.replace(home, "~")
    return re.sub(r"\x00(\d+)\x00", lambda m: keep[int(m.group(1))], s)


def mask_line(line: str, home: str | None = None) -> str:
    """One line: withheld completely or with its values masked. Never raises."""
    line = line.rstrip("\r\n")
    if is_sensitive_line(line):
        return PLACEHOLDER
    return _mask_values(line, home)


class StreamMasker:
    """Line-by-line masking with the multi-line rule (state = "the previous line was withheld and its value may continue")."""

    def __init__(self, home: str | None = None) -> None:
        self.home = home
        self.carry = False
        self.quote_open = False  # a quote opened in a withheld line is still open: following lines belong to the value

    def feed(self, raw: str) -> str:
        line = raw.rstrip("\r\n")
        bare_key = bool(_BARE_KEY.search(line))
        if self.carry or bare_key or is_sensitive_line(line):
            # the value may continue on the next line: the line ends with ':' '=' or a backslash, or a quote stays open
            if line.count('"') % 2 == 1 or line.count("'") % 2 == 1:
                self.quote_open = not self.quote_open
            self.carry = bare_key or self.quote_open or bool(re.search(r"[:=\\]\s*$", line))
            return PLACEHOLDER
        self.carry = False
        self.quote_open = False
        return _mask_values(line, self.home)


def mask_text(text: str, home: str | None = None) -> str:
    """A block of text (several lines). Continuation lines of withheld lines are withheld as well."""
    m = StreamMasker(home)
    out = [m.feed(line) for line in text.splitlines()]
    return "\n".join(out) + ("\n" if text.endswith("\n") else "")


def contains_secret_shape(text: str) -> bool:
    """Heuristic self-check for reports: does the text still look like it carries credentials?"""
    return bool(re.search(rf"(?i)(?:{SECRET_KEY})\s*[:=]\s*[^\s<]", text) or URL_CREDS.search(text) or JWT.search(text) or "registering with nick" in text.lower())
