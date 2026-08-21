//! One vocabulary for the JSON surfaces, so the lists that key off command strings cannot
//! disagree.
//!
//! There were four: `pipe::dispatch`'s match, `dispatch_single`'s match, the classification
//! that decides which commands owe a change report, and the one that decides which leave the
//! baseline behind them. Nothing connected them, and they had already drifted:
//! `mutates_page` classified `tap`, `double_click` and `double-click` as page-mutating and
//! neither dispatcher had an arm for any of the three, so all three were unreachable.
//!
//! That drift was harmless in the direction it happened. The other direction is not: a
//! dispatchable name missing from the classification runs, mutates the page, and answers
//! `ok:true` with no `changed`, no `delta` and no verdict, which is how a read answers. The
//! caller reads a write as a read.
//!
//! Here the spelling is resolved to an identity once, and every question downstream is asked
//! of the identity. Adding a command is one variant, and the compiler then refuses to build
//! until both dispatchers handle it and both classifications answer for it.

use serde_json::Value;

/// A command name that no verb answers to.
#[derive(Debug)]
pub struct UnknownVerb;

/// Declares the vocabulary once and derives the enum, the spelling table and the parser from
/// it, so none of the three can fall out of step with the others.
///
/// `requires_change_report` is deliberately NOT generated. It is the one semantic judgement
/// here, and it belongs in source where it can be read and argued with rather than inside a
/// table. The compiler still enforces that it answers for every variant.
macro_rules! verbs {
    ($( $variant:ident => [ $($name:literal),+ $(,)? ] ),+ $(,)?) => {
        /// What a JSON command is, independent of how it was spelled.
        #[derive(Debug, Clone, Copy, PartialEq, Eq)]
        pub enum PipeVerb {
            $($variant),+
        }

        impl PipeVerb {
            /// Every verb. Used by the tests that must not miss one, and by anything that
            /// needs to enumerate the vocabulary rather than answer about one word.
            #[cfg_attr(not(test), allow(dead_code))]
            pub const ALL: &'static [Self] = &[$(Self::$variant),+];

            /// Every spelling this verb answers to. The one place an alias is written.
            #[cfg_attr(not(test), allow(dead_code))]
            pub const fn names(self) -> &'static [&'static str] {
                match self {
                    $(Self::$variant => &[$($name),+]),+
                }
            }
        }

        impl std::str::FromStr for PipeVerb {
            type Err = UnknownVerb;

            /// A direct match rather than a scan of `ALL`, so two verbs claiming the same
            /// spelling is a compile error (`unreachable_patterns`) instead of a silent
            /// first-one-wins.
            fn from_str(s: &str) -> Result<Self, Self::Err> {
                match s {
                    $($($name)|+ => Ok(Self::$variant),)+
                    _ => Err(UnknownVerb),
                }
            }
        }
    };
}

verbs! {
    Assert => ["assert"],
    Back => ["back"],
    Batch => ["batch"],
    Check => ["check"],
    Click => ["click"],
    Console => ["console"],
    Dblclick => ["dblclick"],
    Diff => ["diff"],
    Download => ["download"],
    Drag => ["drag"],
    Eval => ["eval"],
    Extract => ["extract"],
    Fill => ["fill"],
    FillAndSubmit => ["fill_and_submit", "fill-and-submit"],
    FillForm => ["fill-form", "fill_form", "fillform"],
    Forward => ["forward"],
    Frame => ["frame"],
    Goto => ["goto"],
    History => ["history"],
    Hover => ["hover"],
    Inspect => ["inspect"],
    NavigateAndRead => ["navigate_and_read", "navigate-and-read"],
    Network => ["network"],
    Pdf => ["pdf"],
    Press => ["press"],
    Read => ["read"],
    Screenshot => ["screenshot"],
    Scroll => ["scroll"],
    Select => ["select"],
    Tabs => ["tabs"],
    Text => ["text"],
    Type => ["type"],
    Uncheck => ["uncheck"],
    Upload => ["upload"],
    Wait => ["wait"],
}

impl PipeVerb {
    /// Whether this verb owes the caller a change report.
    ///
    /// Named for the obligation, not for the behaviour. "Can it move the page" is a different
    /// question and answering it here is how `eval` came to be misfiled: it runs arbitrary
    /// caller JavaScript, so it plainly can move the page, and it still owes no report,
    /// because a report costs a settle plus a tree read and `eval` is also the documented way
    /// to read structured data out of a page. What it owes instead is
    /// [`Self::invalidates_baseline`].
    ///
    /// `goto`, `back` and `forward` navigate and owe nothing either: the caller navigated on
    /// purpose, and a truncated slice of the destination is neither a delta nor a usable
    /// snapshot.
    ///
    /// No wildcard arm, and the lint below makes that structural rather than a request. A `_`
    /// here would answer "no report" for every command added afterwards, silently, which is
    /// the exact failure this module exists to prevent.
    #[deny(clippy::wildcard_enum_match_arm)]
    pub const fn requires_change_report(self) -> bool {
        match self {
            Self::Check
            | Self::Click
            | Self::Dblclick
            | Self::Drag
            | Self::Fill
            | Self::FillAndSubmit
            | Self::FillForm
            | Self::Hover
            | Self::Press
            | Self::Scroll
            | Self::Select
            | Self::Type
            | Self::Uncheck
            | Self::Upload => true,

            Self::Assert
            | Self::Back
            | Self::Batch
            | Self::Console
            | Self::Diff
            | Self::Download
            | Self::Eval
            | Self::Extract
            | Self::Forward
            | Self::Frame
            | Self::Goto
            | Self::History
            | Self::Inspect
            | Self::NavigateAndRead
            | Self::Network
            | Self::Pdf
            | Self::Read
            | Self::Screenshot
            | Self::Tabs
            | Self::Text
            | Self::Wait => false,
        }
    }

    /// Whether this verb leaves the stored snapshot no longer describing the page.
    ///
    /// Takes the arguments as well as the identity, because `extract` only moves the page when
    /// it was asked to scroll, and charging the ordinary read for that would cost every caller
    /// a report to fix a claim nobody made.
    ///
    /// The snapshot is flagged, never dropped: `diff` asks what changed since the caller last
    /// looked and an `eval`'s work belongs in that answer, while an action's change report
    /// asks what THAT action did and the same work must not appear there. See
    /// `session::PageSession::baseline_moved`.
    #[deny(clippy::wildcard_enum_match_arm)]
    pub fn invalidates_baseline(self, cmd: &Value) -> bool {
        match self {
            // Arbitrary caller-supplied JavaScript. Can click, submit, navigate or rewrite
            // the DOM, and answers for none of it.
            Self::Eval => true,
            // `--scroll` drives the page to the bottom to trigger lazy loading, then scrolls
            // back. The position is restored; the content it loaded is not.
            Self::Extract => cmd.get("scroll").and_then(Value::as_bool).unwrap_or(false),

            // `inspect --scroll` scrolls too and needs nothing: it writes the snapshot it just
            // took as the new baseline. `goto`, `back` and `forward` replace the document, so
            // the stored loaderId stops matching and the comparison answers
            // `document_replaced` rather than believing a diff across two different pages.
            Self::Assert
            | Self::Back
            | Self::Batch
            | Self::Check
            | Self::Click
            | Self::Console
            | Self::Dblclick
            | Self::Diff
            | Self::Download
            | Self::Drag
            | Self::Fill
            | Self::FillAndSubmit
            | Self::FillForm
            | Self::Forward
            | Self::Frame
            | Self::Goto
            | Self::History
            | Self::Hover
            | Self::Inspect
            | Self::NavigateAndRead
            | Self::Network
            | Self::Pdf
            | Self::Press
            | Self::Read
            | Self::Screenshot
            | Self::Scroll
            | Self::Select
            | Self::Tabs
            | Self::Text
            | Self::Type
            | Self::Uncheck
            | Self::Upload
            | Self::Wait => false,
        }
    }
}


#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    #[test]
    fn every_spelling_parses_back_to_its_own_verb() {
        for verb in PipeVerb::ALL {
            for name in verb.names() {
                assert_eq!(
                    name.parse::<PipeVerb>().ok(),
                    Some(*verb),
                    "{name} should resolve to {verb:?}"
                );
            }
        }
    }

    #[test]
    fn no_verb_declares_an_empty_spelling_list() {
        for verb in PipeVerb::ALL {
            assert!(!verb.names().is_empty(), "{verb:?} answers to nothing");
        }
    }

    #[test]
    fn no_two_verbs_claim_the_same_spelling() {
        let mut seen: HashSet<&str> = HashSet::new();
        for verb in PipeVerb::ALL {
            for name in verb.names() {
                assert!(seen.insert(name), "{name} is claimed twice, second by {verb:?}");
            }
        }
    }

    #[test]
    fn an_unknown_word_is_refused() {
        assert!("frobnicate".parse::<PipeVerb>().is_err());
        // The three that `mutates_page` used to classify and no dispatcher accepted. They are
        // CLI-only clap aliases, and pipe takes none of clap's convenience aliases.
        for dead in ["tap", "double_click", "double-click"] {
            assert!(dead.parse::<PipeVerb>().is_err(), "{dead} is not a pipe verb");
        }
    }

    #[test]
    fn extract_only_moves_the_page_when_asked_to_scroll() {
        let plain = serde_json::json!({"cmd": "extract"});
        let scrolling = serde_json::json!({"cmd": "extract", "scroll": true});
        assert!(!PipeVerb::Extract.invalidates_baseline(&plain));
        assert!(PipeVerb::Extract.invalidates_baseline(&scrolling));
    }

    #[test]
    fn a_verb_that_reports_never_also_flags_the_baseline() {
        // The two obligations are alternatives. A verb that answers for itself has already
        // refreshed the baseline by the time it returns; flagging it as well would cost the
        // next action a needless re-read.
        let empty = serde_json::json!({});
        for verb in PipeVerb::ALL {
            assert!(
                !(verb.requires_change_report() && verb.invalidates_baseline(&empty)),
                "{verb:?} claims both obligations"
            );
        }
    }
}
