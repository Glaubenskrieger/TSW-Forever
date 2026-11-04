//! Central masking of secrets for everything the launcher prints, records or hands to a front end.
//!
//! Why: the original client writes its chat registration (nick, user, real name and a `password:` field; whether the value is a
//! session token or a permanent password is unknown, so it is treated as a permanent one) to its log and to its stdout. The client
//! cannot be changed; what the launcher forwards or records can. The same rules exist in `scripts/secretmask.py` for the test tools.
//!
//! Layers: a line that looks like it carries credentials, tokens, cookies or names is **withheld** completely (conservative on
//! purpose); in kept lines credentials in URLs and the values of secret-like `key=value` pairs are **masked**; a withheld line that
//! ends in `:` `=` `\` or leaves a quote open also takes the next line with it ([`StreamMasker`]).
//! Limits: line based text only; a secret without any recognisable key or context can pass; the client's own `ClientLog.txt` is not
//! rewritten by the launcher and stays private.

pub const PLACEHOLDER: &str = "<line withheld: may contain credentials or names>";

/// Key words that make a line "secret-like". They count as WORDS of the line (split at every non-alphanumeric character; a plural
/// `s` and compound keys such as `client_secret` or `access_token` count, `pass-word` too), so the game name (`TheSecretWorld`,
/// `secretworld.com`) does not match. Leetspeak digits 4/@ and 0 are read as letters (`p4ssw0rd`).
const WORDS: &[&str] = &[
    "password",
    "passwd",
    "passphrase",
    "passcode",
    "pwd",
    "secret",
    "token",
    "cookie",
    "bearer",
    "authorization",
    "loginkey",
    "statindex",
    "ticket",
    "credential",
    "apikey",
    "accesskey",
    "privatekey",
    "sessionid",
    "sessionkey",
    "nick",
    "nickname",
    "registering",
    "oauth",
    "jwt",
];
/// Pairs of neighbouring words that form a key (`pass word`, `api key`, `login key`, `stat index`, `session id`).
const PAIRS: &[(&str, &str)] = &[("pass", "word"), ("api", "key"), ("access", "key"), ("private", "key"), ("login", "key"), ("stat", "index"), ("session", "id"), ("session", "key")];

fn leet(word: &str) -> String {
    word.chars()
        .map(|c| match c.to_ascii_lowercase() {
            '4' | '@' => 'a',
            '0' => 'o',
            o => o,
        })
        .collect()
}

fn key_word(w: &str) -> bool {
    let w = w.strip_suffix('s').filter(|x| x.len() > 3).unwrap_or(w);
    WORDS.contains(&w) || WORDS.iter().any(|k| ["secret", "token", "password", "passwd", "apikey", "cookie"].contains(k) && w.len() > k.len() && w.ends_with(k) && !w.ends_with("world"))
}

/// IRC-style registration commands the chat connection speaks: `PASS x`, `NICK x`, `USER a b c :real`.
fn irc_command(line: &str) -> bool {
    let t = line.trim_start_matches(|c: char| !c.is_ascii_alphanumeric());
    let mut it = t.split_whitespace();
    matches!((it.next(), it.next()), (Some(c), Some(_)) if ["PASS", "NICK", "USER", "OPER"].iter().any(|k| c.eq_ignore_ascii_case(k)) && c.chars().all(|ch| ch.is_ascii_uppercase()) || c == "pass")
}

/// True if the whole line must be withheld.
pub fn is_sensitive_line(line: &str) -> bool {
    // words split at non-alphanumerics, with `@` kept inside a word for leetspeak
    let words: Vec<String> = line.split(|c: char| !(c.is_ascii_alphanumeric() || c == '@')).filter(|w| !w.is_empty()).map(leet).collect();
    words.iter().any(|w| key_word(w)) || words.windows(2).any(|p| PAIRS.iter().any(|(a, b)| p[0] == *a && p[1] == *b)) || irc_command(line)
}

/// `scheme://user:pw@host/x` → `scheme://<masked>@host/x` (also `scheme://user@host`).
pub fn mask_url_credentials(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(i) = rest.find("://") {
        let (head, tail) = rest.split_at(i + 3);
        out.push_str(head);
        let end = tail.find(|c: char| c == '/' || c.is_whitespace()).unwrap_or(tail.len());
        match tail[..end].rfind('@') {
            Some(at) => {
                out.push_str("<masked>");
                rest = &tail[at..];
            }
            None => rest = tail,
        }
    }
    out.push_str(rest);
    out
}

fn mask_values(line: &str) -> String {
    mask_url_credentials(line)
}

/// One line, withheld or with its values masked.
pub fn mask_line(line: &str) -> String {
    let line = line.trim_end_matches(['\r', '\n']);
    if is_sensitive_line(line) {
        PLACEHOLDER.to_string()
    } else {
        mask_values(line)
    }
}

/// Line-by-line masking with the multi-line rule.
#[derive(Default)]
pub struct StreamMasker {
    carry: bool,
    /// A quote opened in a withheld line is still open: the following lines belong to the value.
    quote_open: bool,
}

impl StreamMasker {
    pub fn feed(&mut self, raw: &str) -> String {
        let line = raw.trim_end_matches(['\r', '\n']);
        let sensitive = is_sensitive_line(line);
        if self.carry || sensitive {
            let t = line.trim_end();
            if line.matches('"').count() % 2 == 1 || line.matches('\'').count() % 2 == 1 {
                self.quote_open = !self.quote_open;
            }
            self.carry = self.quote_open || t.ends_with(':') || t.ends_with('=') || t.ends_with('\\');
            return PLACEHOLDER.to_string();
        }
        self.carry = false;
        self.quote_open = false;
        mask_values(line)
    }
}

/// A block of text (several lines).
pub fn mask_text(text: &str) -> String {
    let mut m = StreamMasker::default();
    let mut out: Vec<String> = text.lines().map(|l| m.feed(l)).collect();
    if text.ends_with('\n') {
        out.push(String::new());
    }
    out.join("\n")
}

/// Copy `from` to `to` line by line through the masker (used for the client's console output). Returns when `from` ends.
pub fn copy_masked<R: std::io::Read, W: std::io::Write>(from: R, mut to: W) -> std::io::Result<()> {
    use std::io::BufRead;
    let mut m = StreamMasker::default();
    let mut reader = std::io::BufReader::new(from);
    let mut buf = Vec::new();
    while reader.read_until(b'\n', &mut buf)? > 0 {
        let line = String::from_utf8_lossy(&buf).into_owned();
        writeln!(to, "{}", m.feed(&line))?;
        to.flush()?;
        buf.clear();
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    // every value below is invented
    const FAKE: &str = "Zx9fakeVALUEqq42";

    #[test]
    fn the_chat_registration_the_client_writes_is_withheld_whatever_the_spelling() {
        let lines = [
            format!("IMPORTANT: ChatClient - chat is connected to chat.example.invalid:7001 and is registering with nick: Nobody, password: {FAKE}, user: nobody, real: N"),
            format!("PASS {FAKE}"),
            "NICK Nobody".to_string(),
            "USER nobody 0 * :Nobody N".to_string(),
            format!("Password={FAKE}"),
            format!("pass-word: {FAKE}"),
            format!("p4ssw0rd {FAKE}"),
            format!("API_KEY={FAKE}"),
            format!("Authorization: Bearer {FAKE}"),
            format!("Set-Cookie: sid={FAKE}"),
            format!("-loginkey {FAKE}"),
            format!("client_secret={FAKE}"),
            format!("accessToken: {FAKE}"),
            format!("session id {FAKE}"),
        ];
        for l in &lines {
            let out = mask_line(l);
            assert_eq!(out, PLACEHOLDER, "{l}");
            assert!(!out.contains(FAKE) && !out.contains("Nobody"));
        }
    }

    #[test]
    fn credentials_in_urls_are_masked_and_the_rest_of_the_line_stays() {
        assert_eq!(mask_line(&format!("fetch https://user:{FAKE}@host.example.invalid/path ok")), "fetch https://<masked>@host.example.invalid/path ok");
        assert_eq!(mask_line("ftp://someone@host.example.invalid/x"), "ftp://<masked>@host.example.invalid/x");
        assert_eq!(mask_line("see https://host.example.invalid/a?b=c@d now"), "see https://host.example.invalid/a?b=c@d now", "an @ after the path is not user info");
        assert_eq!(mask_line("two http://a:b@x/ and http://c:d@y/"), "two http://<masked>@x/ and http://<masked>@y/");
    }

    #[test]
    fn technical_lines_are_not_touched() {
        for l in [
            "[2026-10-30 16:10:53Z #0] [ID:0] IMPORTANT: Main - Universe is a live universe",
            "0024:trace:loaddll:build_module Loaded L\"C:\\\\windows\\\\system32\\\\d3dcompiler_47.dll\" at 78BE0000: native",
            "0024:fixme:d3dx:D3DXCreateTextureFromFileInMemoryEx Couldn't find suitable texture parameters.",
            "ERROR: Cheetah - Failed to compile shader (0x80004001)) \"colp_model.psh\":fxc /Tps_5_0 colp_model.psh /DSKIN=1",
            "ERROR: Cheetah - colp_model.vsh:861:47: E5017: Aborting due to not yet implemented feature: Multiple register() reservations.",
            "Failed to create shader input layout",
            "client: /home/someone/proj/tsw-copy/TheSecretWorldDX11.exe",
            "Connecting to TerritoryManager at dm01-nj4.secretworld.com:7002.",
            "IMPORTANT: FlowControl - State changing from 'LoggedOut' to 'ConnectingToUniverse'",
        ] {
            assert_eq!(mask_line(l), l);
        }
    }

    #[test]
    fn a_value_on_the_next_line_is_withheld_with_its_key() {
        let out = mask_text(&format!("keep 1\npassword:\n  {FAKE}\nkeep 2\nsecret = \\\n{FAKE}\nkeep 3\n"));
        assert!(!out.contains(FAKE));
        assert_eq!(out.lines().filter(|l| l.starts_with("keep")).count(), 3, "{out}");
        let q = mask_text(&format!("password: \"start\n{FAKE}\"\nok"));
        assert!(!q.contains(FAKE) && q.ends_with("ok"));
    }

    #[test]
    fn the_console_copy_masks_a_stream_and_keeps_every_other_line() {
        let input = format!("line one\nchat registering with nick: Nobody, password: {FAKE}\nline three\r\nPASS {FAKE}\nlast without newline");
        let mut out = Vec::new();
        copy_masked(input.as_bytes(), &mut out).unwrap();
        let text = String::from_utf8(out).unwrap();
        assert!(!text.contains(FAKE) && !text.contains("Nobody"));
        let lines: Vec<&str> = text.lines().collect();
        assert_eq!((lines[0], lines[2], lines[4]), ("line one", "line three", "last without newline"));
        assert_eq!(lines.len(), 5);
    }
}
