use super::*;

// ---------------------------------------------------------------------------
// Slice 3: secrets at deploy time (ADR 0195)
// ---------------------------------------------------------------------------

/// Where a secret's name came from — the plan's `declared`/`supplied` mark.
///
/// The distinction is the **floor, not a census** contract made visible
/// (ADR 0195 D2): `declared` is a name the compiler proved this Worker reads,
/// `supplied` is one only the user knows about. A reader must be able to tell
/// which of the two they are looking at, because the compiler's silence about a
/// `bynk.Secrets` name is not evidence that no such name exists.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub(crate) enum Origin {
    /// The compiler proved a handler reads it *and* that its absence is
    /// fail-closed — an `actor`'s auth secret. Required: no value is an error.
    Declared,
    /// The compiler proved a handler reads it, but `Secrets.get` returns
    /// `Option`, so absence is a legitimate handled outcome (ADR 0196 D3).
    /// Advisory: no value warns.
    Read,
    /// The user named it. The compiler knows nothing about it either way.
    Supplied,
}

impl Origin {
    /// Is a missing value fatal?
    ///
    /// Only for `Declared`. This is the whole of ADR 0196 D3, and the reason the
    /// increment does **not** promote a read into the required class: an unset
    /// auth secret 401s every request, while an unset `Secrets.get` name is a
    /// `None` the program may be entirely happy about — erroring on it would
    /// break a legal program.
    pub(crate) fn required(self) -> bool {
        matches!(self, Origin::Declared)
    }

    pub(crate) fn label(self) -> &'static str {
        match self {
            Origin::Declared => "declared",
            Origin::Read => "read",
            Origin::Supplied => "supplied",
        }
    }
}

/// One secret this run intends to set on one context.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct WantedSecret {
    pub(crate) name: String,
    pub(crate) origin: Origin,
}

/// The user's secret input.
///
/// Names and values are separate questions (ADR 0195 D3). The file supplies
/// both; `--secret` supplies a name alone; the environment supplies values for
/// names **already known** and is never scanned for names — sweeping `env` into
/// Cloudflare would exfiltrate the user's whole shell.
#[derive(Debug, Default, Clone)]
pub(crate) struct SecretSource {
    /// `--secrets-file` — names and values.
    file: BTreeMap<String, String>,
    /// `--secret NAME` — names only.
    named: BTreeSet<String>,
}

impl SecretSource {
    /// Read the source the options describe. Failing to read a named file is an
    /// error rather than an empty source: the user pointed at it, so silently
    /// proceeding with no values would surface later as a missing-secret error
    /// naming the wrong cause.
    pub(crate) fn read(opts: &DeployOptions) -> Result<Self, String> {
        let file = match &opts.secrets_file {
            Some(path) => {
                let text = std::fs::read_to_string(path)
                    .map_err(|e| format!("could not read {}: {e}", path.display()))?;
                parse_secrets_file(&text)
                    .map_err(|e| format!("could not read {}: {e}", path.display()))?
            }
            None => BTreeMap::new(),
        };
        Ok(Self {
            file,
            named: opts.secrets.iter().cloned().collect(),
        })
    }
}

/// Parse a dotenv-style `NAME=value` file.
///
/// Deliberately thin — `deploy` moves values, it does not store them, and the
/// source is an input rather than a vault this track owns. `#` comments, blank
/// lines, an optional `export ` prefix, and one layer of matching quotes around
/// a value that would otherwise lose its spacing.
///
/// A malformed line is an **error naming its number**, never a skip: a silently
/// dropped line is a secret that does not get set, which surfaces in production
/// as a 401 rather than here as a typo.
fn parse_secrets_file(text: &str) -> Result<BTreeMap<String, String>, String> {
    let mut out = BTreeMap::new();
    for (i, raw) in text.lines().enumerate() {
        let line = raw.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let line = line.strip_prefix("export ").unwrap_or(line).trim_start();
        let Some((name, value)) = line.split_once('=') else {
            return Err(format!("line {}: expected `NAME=value`", i + 1));
        };
        let name = name.trim();
        // Not a full Cloudflare naming rule — that is Cloudflare's to enforce,
        // and inventing one here would reject a name the platform accepts. This
        // catches the shapes that are unambiguously a typo in *this* file.
        if name.is_empty() || name.split_whitespace().count() > 1 {
            return Err(format!(
                "line {}: `{name}` is not a usable secret name",
                i + 1
            ));
        }
        if out
            .insert(name.to_string(), unquote(value.trim()))
            .is_some()
        {
            return Err(format!("line {}: `{name}` is set twice", i + 1));
        }
    }
    Ok(out)
}

/// Strip one layer of matching quotes, so a value with meaningful spacing can
/// survive the line trim.
fn unquote(value: &str) -> String {
    for quote in ['"', '\''] {
        if value.len() >= 2 && value.starts_with(quote) && value.ends_with(quote) {
            return value[1..value.len() - 1].to_string();
        }
    }
    value.to_string()
}

/// Which secrets this context wants set, and where each name came from.
///
/// Declared names are this context's own — the compiler proved *this* Worker
/// reads them. Supplied names go to **every** context in the run: nothing tells
/// `deploy` which contexts read a `bynk.Secrets` name, so the only available
/// answers are "all of them" or "none", and none would make `--secrets-file`
/// useless. The plan lists them per context so that spread is visible rather
/// than implied (ADR 0195 D2).
///
/// A name that is both declared and supplied is marked `declared`: the
/// compiler's knowledge is the more informative label, and it is the reason a
/// missing value is an error rather than a shrug.
pub(crate) fn wanted_secrets(
    declared: &[String],
    read: &[String],
    source: &SecretSource,
) -> Vec<WantedSecret> {
    let mut marks: BTreeMap<String, Origin> = BTreeMap::new();
    for name in source.file.keys().chain(source.named.iter()) {
        marks.insert(name.clone(), Origin::Supplied);
    }
    // Read beats supplied, and declared beats both: the marks are ordered by how
    // much the compiler knows, and the strongest thing known about a name is the
    // most useful label for it. `declared` last because it is the only one that
    // makes a missing value fatal.
    for name in read {
        marks.insert(name.clone(), Origin::Read);
    }
    for name in declared {
        marks.insert(name.clone(), Origin::Declared);
    }
    marks
        .into_iter()
        .map(|(name, origin)| WantedSecret { name, origin })
        .collect()
}

/// What the run will do with one secret.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SecretAction {
    Set,
    Overwrite,
    SkipPresent,
}

/// May a secret we cannot find a value for be left alone rather than failing the
/// deploy? Pure, so the rule is tested without an account.
///
/// Only where **both** hold: presence is unknown (the account could not be
/// asked), and the Worker has been live before. That combination is far more
/// likely to mean "already set, and the user had no reason to supply it again"
/// — the common CI redeploy, no `--secrets-file`, no TTY — than "genuinely
/// missing". Failing there would block a deploy that works, on the strength of a
/// *read* failure in a check that is advisory by design (ADR 0195 D4).
///
/// A first deploy gets no benefit of the doubt: its Worker is new, so an
/// unresolvable declared secret really is missing. And where presence *is*
/// known, the answer is authoritative — a secret we know to be absent, with no
/// value to set, is a real failure that must be named.
fn tolerate_unresolvable(present: Option<&BTreeSet<String>>, first_deploy: bool) -> bool {
    present.is_none() && !first_deploy
}

/// The set-if-absent rule (ADR 0195 D4). Pure, so the rule is tested without an
/// account.
///
/// `present` is `None` when the account could not be asked — a Worker that does
/// not exist yet, or an auth/network failure. Both are read as "assume nothing
/// is set and try": a first deploy genuinely has no secrets, and a real auth
/// failure then surfaces as the `secret put`'s own complaint rather than as a
/// diagnosis this function invented (the `queue_exists` posture).
fn secret_action(name: &str, present: Option<&BTreeSet<String>>, force: bool) -> SecretAction {
    match (present.is_some_and(|p| p.contains(name)), force) {
        (false, _) => SecretAction::Set,
        (true, true) => SecretAction::Overwrite,
        (true, false) => SecretAction::SkipPresent,
    }
}

/// The non-interactive half of D3's precedence: the file, else the environment.
/// `None` means only a prompt is left. Pure (the environment is read by the
/// caller), so the precedence is tested without touching the process env.
fn value_from(name: &str, source: &SecretSource, from_env: Option<String>) -> Option<String> {
    source.file.get(name).cloned().or(from_env)
}

/// Everything the secret step needs, threaded as one value rather than as four
/// more parameters on an already-long list.
pub(crate) struct Secrets<'a> {
    pub(crate) source: &'a SecretSource,
    pub(crate) force: bool,
    /// Values resolved earlier in this run, so two contexts wanting the same
    /// name prompt once rather than once each.
    ///
    /// In memory only, and dropped with the run: holding a value long enough to
    /// hand it to wrangler is unavoidable, but nothing here is ever written —
    /// the ledger records no secret, not even its presence (ADR 0195 D1).
    pub(crate) resolved: &'a mut BTreeMap<String, String>,
}

/// Decide what to set and resolve every value — the non-mutating half.
///
/// Returns the `(name, value)` pairs [`apply_secrets`] will put. Split from the
/// put so that **every failure a user can act on happens before anything is
/// pushed**, on both sides of D6's straddle: a first deploy pushes before it
/// sets, so resolving lazily there would surface a missing value only once a
/// live Worker existed, 401ing every request. Nothing here touches Cloudflare
/// except the advisory presence read.
#[allow(clippy::too_many_arguments)]
pub(crate) fn prepare_secrets(
    provenance: &Provenance,
    worker_dir: &Path,
    worker: &str,
    declared: &Resources,
    secrets: &mut Secrets<'_>,
    first_deploy: bool,
    environment: &str,
) -> Result<Vec<(String, String)>, DeployFailure> {
    let wanted = wanted_secrets(
        &declared.declared_secrets,
        &declared.read_secrets,
        secrets.source,
    );
    // Said once per context, before any of it: a reader who takes the lines
    // below for the whole story would be wrong, and this is the only place that
    // can tell them (ADR 0196 D2).
    if !declared.reads_complete {
        eprintln!(
            "bynk: `{worker}` names at least one secret with a computed expression, so the list below is not everything it reads."
        );
    }
    if wanted.is_empty() {
        return Ok(Vec::new());
    }
    // Asked live, never recorded: presence is the only observable Cloudflare
    // offers (it does not return values), and a recorded answer could only ever
    // be a stale one (ADR 0195 D1/D4). Asked *before* the push on both paths —
    // our own `wrangler deploy` does not change which secrets are set, so one
    // answer serves the whole step, and `secret list` has no draft-Worker path
    // to trip over on a first deploy.
    let present = list_secrets(provenance, worker_dir, environment);
    let mut prepared = Vec::new();
    for want in &wanted {
        match secret_action(&want.name, present.as_ref(), secrets.force) {
            SecretAction::SkipPresent => {
                eprintln!(
                    "bynk: secret `{}` is already set on `{worker}`, skipping — use --force to overwrite",
                    want.name
                );
                continue;
            }
            SecretAction::Set | SecretAction::Overwrite => {}
        }
        match resolve_secret_value(&want.name, want.origin, worker, secrets) {
            Ok(value) => prepared.push((want.name.clone(), value)),
            // Advisory by type (ADR 0196 D3): `Secrets.get` returns `Option`, so
            // a read with no value is a `None` the program may be entirely happy
            // about. Say what will happen and carry on — failing here would
            // refuse to deploy a legal program over a secret it never needed.
            Err(_) if !want.origin.required() => {
                eprintln!(
                    "bynk: `{worker}` reads the secret `{}`, but no value was supplied — it will see None.",
                    want.name
                );
                eprintln!(
                    "  Supply it with --secrets-file or --secret {}, if it is meant to be set.",
                    want.name
                );
            }
            Err(_) if tolerate_unresolvable(present.as_ref(), first_deploy) => {
                eprintln!(
                    "bynk: could not ask Cloudflare which secrets `{worker}` has, and no value for `{}` was supplied — leaving it as it is.",
                    want.name
                );
                eprintln!(
                    "  If it is not set, `{worker}` will answer 401. Check with `wrangler secret list`, or supply a value to set it."
                );
            }
            Err(e) => return Err(e),
        }
    }
    Ok(prepared)
}

/// Put each prepared secret. The mutating half, and deliberately the whole of
/// it: everything that can fail for a reason the user can fix — a missing value,
/// a malformed file — has already happened by the time this runs.
pub(crate) fn apply_secrets(
    provenance: &Provenance,
    worker_dir: &Path,
    worker: &str,
    prepared: &[(String, String)],
    environment: &str,
) -> Result<(), DeployFailure> {
    for (name, value) in prepared {
        set_secret(provenance, worker_dir, name, value, environment).map_err(|e| {
            DeployFailure::driver(format!(
                "could not set the secret `{name}` on `{worker}`: {e}"
            ))
        })?;
    }
    Ok(())
}

/// The value for one secret: this run's cache, then the file, then the
/// environment, then a prompt (ADR 0195 D3).
///
/// A name with no value anywhere is a **hard error naming it** when there is no
/// terminal to ask — never a blank. A blank would be worse than the failure it
/// replaces: the deploy would report success and the Worker would 401 every
/// request, with nothing to read that says why.
fn resolve_secret_value(
    name: &str,
    origin: Origin,
    worker: &str,
    secrets: &mut Secrets<'_>,
) -> Result<String, DeployFailure> {
    if let Some(cached) = secrets.resolved.get(name) {
        return Ok(cached.clone());
    }
    let from_env = std::env::var(name).ok();
    let value = match value_from(name, secrets.source, from_env) {
        Some(value) => value,
        None => prompt_for_secret(name)
            .ok_or_else(|| DeployFailure::driver(missing_secret_message(name, origin, worker)))?,
    };
    secrets.resolved.insert(name.to_string(), value.clone());
    Ok(value)
}

/// Ask for a value, when there is a terminal to ask.
///
/// `None` when there is no TTY (CI, a piped session) — the caller turns that
/// into the named error. Read from the terminal without echoing would be better
/// still; v1 does not, so the prompt says so rather than implying secrecy it
/// does not provide.
fn prompt_for_secret(name: &str) -> Option<String> {
    if !io::stdin().is_terminal() {
        return None;
    }
    eprint!("Secret `{name}` (input is visible): ");
    let _ = io::stderr().flush();
    let mut answer = String::new();
    if io::stdin().read_line(&mut answer).is_err() {
        return None;
    }
    let answer = answer.trim_end_matches(['\n', '\r']).to_string();
    (!answer.is_empty()).then_some(answer)
}

/// Why a secret could not be resolved, in the vocabulary of where its name came
/// from — the two origins are missing for different reasons and want different
/// remedies.
fn missing_secret_message(name: &str, origin: Origin, worker: &str) -> String {
    match origin {
        Origin::Declared => format!(
            "`{worker}` declares the secret `{name}` (an actor's `auth` secret) but no value was supplied — \
             pass it in --secrets-file, or set {name} in the environment. \
             Deploying without it would answer every request with 401."
        ),
        // Reached only when a read is *required*, which it never is — kept
        // total rather than unreachable!(), since the compiler cannot know that
        // and a panic here would be a poor way to learn otherwise.
        Origin::Read | Origin::Supplied => format!(
            "the secret `{name}` was named but no value was supplied — \
             set {name} in the environment, or give it a value in --secrets-file."
        ),
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::deploy::config::tests::project;

    fn names(v: &[&str]) -> Vec<String> {
        v.iter().map(|s| s.to_string()).collect()
    }

    /// A user secret input literal: `source(&[("A", "v")], &["B"])` is a
    /// `--secrets-file` carrying `A=v` plus a `--secret B`.
    pub(crate) fn source(file: &[(&str, &str)], named: &[&str]) -> SecretSource {
        SecretSource {
            file: file
                .iter()
                .map(|(k, v)| (k.to_string(), v.to_string()))
                .collect(),
            named: names(named).into_iter().collect(),
        }
    }

    // ---- #602 slice 3: secrets at deploy (ADR 0195) --------------------

    #[test]
    fn a_secrets_file_is_read_as_names_and_values() {
        let parsed = parse_secrets_file(
            r#"
# a comment
API_KEY=sk_live_abc

export EXPORTED=fine
QUOTED="  spaced  "
SINGLE='single'
EMPTY=
WITH_EQUALS=a=b
"#,
        )
        .expect("a well-formed file parses");
        assert_eq!(
            parsed.get("API_KEY").map(String::as_str),
            Some("sk_live_abc")
        );
        assert_eq!(
            parsed.get("EXPORTED").map(String::as_str),
            Some("fine"),
            "an `export ` prefix is the shape a shell-sourced file already has"
        );
        assert_eq!(
            parsed.get("QUOTED").map(String::as_str),
            Some("  spaced  "),
            "quotes are what let a value keep spacing the line trim would eat"
        );
        assert_eq!(parsed.get("SINGLE").map(String::as_str), Some("single"));
        assert_eq!(
            parsed.get("EMPTY").map(String::as_str),
            Some(""),
            "an empty value is the user's to mean; only a missing *name* is an error"
        );
        assert_eq!(
            parsed.get("WITH_EQUALS").map(String::as_str),
            Some("a=b"),
            "only the first `=` separates — a value may contain more"
        );
        assert!(!parsed.contains_key("# a comment"));
    }

    #[test]
    fn a_malformed_secrets_line_is_an_error_naming_its_number() {
        // The consequence that makes this worth failing on: a silently skipped
        // line is a secret that never gets set, which surfaces in production as
        // a 401 rather than here as a typo.
        let err = parse_secrets_file("GOOD=1\nnonsense\n").expect_err("a bare word is not a pair");
        assert!(err.contains("line 2"), "{err}");
        let err = parse_secrets_file("A B=1\n").expect_err("a spaced name is a typo");
        assert!(err.contains("line 1"), "{err}");
        let err = parse_secrets_file("=1\n").expect_err("no name at all");
        assert!(err.contains("line 1"), "{err}");
        let err = parse_secrets_file("A=1\nA=2\n").expect_err("set twice");
        assert!(err.contains("line 2") && err.contains('A'), "{err}");
    }

    #[test]
    fn the_wanted_set_is_declared_union_supplied_and_declared_wins_the_mark() {
        let src = source(&[("STRIPE_KEY", "v")], &["PROBE_TOKEN"]);
        let wanted = wanted_secrets(&names(&["AUTH_JWT_SECRET"]), &[], &src);
        assert_eq!(
            wanted,
            vec![
                WantedSecret {
                    name: "AUTH_JWT_SECRET".into(),
                    origin: Origin::Declared
                },
                WantedSecret {
                    name: "PROBE_TOKEN".into(),
                    origin: Origin::Supplied
                },
                WantedSecret {
                    name: "STRIPE_KEY".into(),
                    origin: Origin::Supplied
                },
            ]
        );

        // A name that is both is `declared`: the compiler's knowledge is the
        // more informative label, and it is why a missing value is an error.
        let both = wanted_secrets(&names(&["SHARED"]), &[], &source(&[("SHARED", "v")], &[]));
        assert_eq!(both[0].origin, Origin::Declared);

        // No input at all: nothing to set, and in particular no invented name.
        assert!(wanted_secrets(&[], &[], &SecretSource::default()).is_empty());
    }

    #[test]
    fn set_if_absent_skips_a_present_secret_unless_forced() {
        let present: BTreeSet<String> = ["THERE".to_string()].into_iter().collect();
        assert_eq!(
            secret_action("THERE", Some(&present), false),
            SecretAction::SkipPresent,
            "the default must not cut a fresh Cloudflare version every deploy"
        );
        assert_eq!(
            secret_action("THERE", Some(&present), true),
            SecretAction::Overwrite
        );
        assert_eq!(
            secret_action("ABSENT", Some(&present), false),
            SecretAction::Set
        );

        // `None` is "could not ask" — a Worker that does not exist yet, or an
        // auth failure. Both mean try: a first deploy genuinely has none, and a
        // real auth failure surfaces as the put's own complaint rather than as
        // a diagnosis invented here.
        assert_eq!(secret_action("THERE", None, false), SecretAction::Set);
    }

    #[test]
    fn an_unaskable_account_does_not_fail_a_redeploy_that_supplies_nothing() {
        // The regression this encodes: `secret_action` reads "could not ask" as
        // `Set`, so a redeploy whose secrets are all already on the account —
        // the common CI shape, no --secrets-file, no TTY — would try to resolve
        // a value it has no reason to have, find none, and fail. A *read*
        // failure in an advisory check would block a deploy that works, and both
        // the code's own comment and ADR 0195 D4 claimed it would not.
        //
        // `tolerate_unresolvable` is the rule: it holds only where presence is
        // unknown *and* the Worker has been live before.
        assert!(
            tolerate_unresolvable(None, false),
            "a redeploy with no presence answer leaves the secret alone"
        );
        // A first deploy gets no benefit of the doubt — its Worker is new, so an
        // unresolvable declared secret really is missing.
        assert!(!tolerate_unresolvable(None, true));
        // Presence known: the answer is authoritative either way, so an
        // unresolvable secret we know to be absent is a real failure.
        let none_present = BTreeSet::new();
        assert!(!tolerate_unresolvable(Some(&none_present), false));
        assert!(!tolerate_unresolvable(Some(&none_present), true));
    }

    #[test]
    fn a_value_comes_from_the_file_before_the_environment() {
        let src = source(&[("A", "from-file")], &[]);
        assert_eq!(
            value_from("A", &src, Some("from-env".into())).as_deref(),
            Some("from-file"),
            "the file is the more specific instruction, so it wins"
        );
        assert_eq!(
            value_from("B", &src, Some("from-env".into())).as_deref(),
            Some("from-env")
        );
        assert_eq!(
            value_from("C", &src, None),
            None,
            "nothing left but a prompt — and, with no terminal, a named error"
        );
    }

    #[test]
    fn wranglers_secret_list_is_read_as_names() {
        // Wrangler's shape (`secret list --format json`, wrangler 4.103).
        assert_eq!(
            parse_secret_list(
                r#"[{"name":"A","type":"secret_text"},{"name":"B","type":"secret_text"}]"#
            ),
            Some(["A".to_string(), "B".to_string()].into_iter().collect()),
        );
        assert_eq!(parse_secret_list("[]"), Some(BTreeSet::new()));
        // Anything unreadable is "could not tell" rather than "none present" —
        // and `secret_action` reads that as "try", which is idempotent.
        assert_eq!(parse_secret_list("not json"), None);
        assert_eq!(parse_secret_list(r#"{"unexpected":"shape"}"#), None);
    }

    #[test]
    fn a_missing_secret_names_itself_and_says_what_it_costs() {
        // The error is the whole mitigation for the silent-blank risk, so it
        // must name the secret and the remedy rather than saying "failed".
        let declared = missing_secret_message("AUTH_JWT_SECRET", Origin::Declared, "api");
        assert!(declared.contains("AUTH_JWT_SECRET"), "{declared}");
        assert!(declared.contains("api"), "{declared}");
        assert!(
            declared.contains("401"),
            "a declared secret's absence is fail-closed — say so: {declared}"
        );
        let supplied = missing_secret_message("STRIPE_KEY", Origin::Supplied, "api");
        assert!(supplied.contains("STRIPE_KEY"), "{supplied}");
        assert!(
            !supplied.contains("401"),
            "a supplied name is not known to gate auth, so do not claim it does: {supplied}"
        );
    }

    #[test]
    fn a_read_is_advisory_and_a_declared_secret_is_not() {
        // ADR 0196 D3, the increment's load-bearing distinction. `Secrets.get`
        // returns `Option`, so an unsupplied read is a `None` the program may be
        // happy about; an unset auth secret 401s every request. Erroring on the
        // first would refuse to deploy a legal program.
        assert!(Origin::Declared.required());
        assert!(!Origin::Read.required());
        assert!(!Origin::Supplied.required());
        assert_eq!(Origin::Read.label(), "read");
    }

    #[test]
    fn the_marks_are_ordered_by_how_much_the_compiler_knows() {
        // A name can be in more than one class. The strongest thing known about
        // it is the most useful label — and `declared` is the only one that
        // makes a missing value fatal, so it must win.
        let src = source(&[("BOTH", "v"), ("SUPPLIED_ONLY", "v")], &[]);
        let wanted = wanted_secrets(&names(&["BOTH"]), &names(&["BOTH", "READ_ONLY"]), &src);
        let mark = |n: &str| {
            wanted
                .iter()
                .find(|w| w.name == n)
                .unwrap_or_else(|| panic!("{n} is wanted"))
                .origin
        };
        assert_eq!(
            mark("BOTH"),
            Origin::Declared,
            "declared beats read and supplied"
        );
        assert_eq!(mark("READ_ONLY"), Origin::Read);
        assert_eq!(mark("SUPPLIED_ONLY"), Origin::Supplied);
    }

    #[test]
    fn the_plan_never_carries_a_secret_value() {
        // ADR 0195 D1's headline guarantee, asserted rather than described: the
        // plan is printed and piped to CI logs, so a value reaching it would be
        // a leak in the most-copied surface `deploy` has.
        const SENTINEL: &str = "sk_live_do_not_leak_me";
        let order = names(&["api"]);
        let declared = project(vec![(
            "api",
            Resources::default().declares(&["AUTH_JWT_SECRET"]),
        )]);
        let plan = derive_plan(
            &order,
            &declared,
            &DeployLock::default(),
            &source(&[("STRIPE_KEY", SENTINEL)], &[]),
            false,
            "default",
        );
        for format in [DeployFormat::Short, DeployFormat::Json] {
            let rendered = plan_report(&plan, format);
            assert!(
                !rendered.contains(SENTINEL),
                "a secret value reached the plan ({format:?}): {rendered}"
            );
            assert!(
                rendered.contains("STRIPE_KEY"),
                "the name is the whole point of the line: {rendered}"
            );
        }
    }

    #[test]
    fn the_ledger_never_carries_a_secret() {
        // The other half of D1: the ledger is a *committed* file, so a value —
        // or even a name — reaching it would be published, not merely logged.
        // The type has no field for one; this pins that as a property rather
        // than an observation about today's struct.
        const SENTINEL: &str = "sk_live_do_not_commit_me";
        let mut lock = DeployLock::default();
        lock.record_deployed("default", "api", Some(Default::default()));
        lock.record_queue("default", "intake");
        let text = toml::to_string_pretty(&lock).expect("the ledger serialises");
        assert!(!text.contains(SENTINEL));
        assert!(
            !text.to_ascii_lowercase().contains("secret"),
            "the ledger records no secret at all — not even its presence: {text}"
        );
    }
}
