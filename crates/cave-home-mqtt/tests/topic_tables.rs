//! Pinning tests for topic names, topic filters and what is built directly
//! on the wildcard matcher: the topic ACL and the bridge topic mapping.
//!
//! The expectations follow OASIS MQTT 5.0 §4.7 (topic names and filters),
//! §4.7.2 (topics beginning with `$`) and §4.8.2 (shared subscriptions);
//! rows marked as such record how this implementation treats input the
//! specification leaves to the caller.

use cave_home_mqtt::bridge::{BridgeConfig, Direction, TopicRule};
use cave_home_mqtt::broker::auth::{AclAction, Authenticator};
use cave_home_mqtt::broker::topic::{
    SharedFilter, parse_shared, topic_matches, valid_topic_filter, valid_topic_name,
};
use cave_home_mqtt::packet::QoS;

type TestResult = Result<(), Box<dyn std::error::Error>>;

// ---- §4.7 wildcard matching -----------------------------------------------------

/// `(filter, topic, matches)`.
const MATCHES: &[(&str, &str, bool)] = &[
    // §4.7.1.2: the multi-level wildcard matches the parent and any depth below.
    ("sport/tennis/player1/#", "sport/tennis/player1", true),
    (
        "sport/tennis/player1/#",
        "sport/tennis/player1/ranking",
        true,
    ),
    (
        "sport/tennis/player1/#",
        "sport/tennis/player1/score/wimbledon",
        true,
    ),
    ("sport/#", "sport", true),
    ("sport/tennis/#", "sport/tennis", true),
    ("sport/tennis/#", "sport/tenni", false),
    ("sport/tennis/#", "sport", false),
    ("#", "sport", true),
    ("#", "sport/tennis", true),
    ("#", "", true),
    ("#", "/", true),
    // §4.7.1.3: the single-level wildcard matches exactly one level.
    ("sport/tennis/+", "sport/tennis/player1", true),
    ("sport/tennis/+", "sport/tennis/player2", true),
    ("sport/tennis/+", "sport/tennis/player1/ranking", false),
    ("sport/+", "sport", false),
    ("sport/+", "sport/", true),
    ("+/+", "/finance", true),
    ("/+", "/finance", true),
    ("+", "/finance", false),
    ("+", "finance", true),
    ("+", "", true),
    ("+/+", "a", false),
    ("+/#", "a", true),
    ("+/tennis/#", "sport/tennis/player1", true),
    ("sport/+/player1", "sport/tennis/player1", true),
    ("sport/+/player1", "sport/tennis/player2", false),
    ("a/+/b", "a//b", true),
    // Without wildcards every level must be equal, empty levels included.
    ("a/b/c", "a/b/c", true),
    ("a/b/c", "a/b/d", false),
    ("a/b", "a/b/c", false),
    ("a/b/c", "a/b", false),
    ("a/b/", "a/b", false),
    ("a/b", "a/b/", false),
    ("/a", "a", false),
    ("a", "/a", false),
    ("a//b", "a//b", true),
    ("a//b", "a/b", false),
    ("", "", true),
    ("A/b", "a/b", false),
    // §4.7.2: a wildcard in the first level does not match a `$` topic.
    ("#", "$SYS/broker/clients", false),
    ("#", "$SYS", false),
    ("#", "$", false),
    ("+", "$SYS", false),
    ("+/monitor/Clients", "$SYS/monitor/Clients", false),
    ("+/#", "$SYS/x", false),
    ("$SYS/#", "$SYS/broker/clients", true),
    ("$SYS/#", "$SYS", true),
    ("$SYS/monitor/+", "$SYS/monitor/Clients", true),
    ("a/#", "a/$x", true),
    ("a/+", "a/$x", true),
    // A `$share/...` filter is compared like any other literal filter.
    ("$share/g/a", "a", false),
    ("$share/g/a", "$share/g/a", true),
    // Wildcard characters inside the topic are ordinary characters.
    ("a/+", "a/#", true),
    ("a/+", "a/+", true),
    ("a/b", "a/+", false),
    ("a/#", "a/#/b", true),
    // Filters the validator refuses: `#` ends the comparison wherever it
    // stands, and a wildcard glued to other characters is a literal.
    ("a/#/b", "a/x/y", true),
    ("sport#", "sport#", true),
    ("sport#", "sport", false),
    ("a+", "ab", false),
    ("a+", "a+", true),
];

#[test]
fn topic_matches_follows_the_wildcard_table() {
    for &(filter, topic, expected) in MATCHES {
        assert_eq!(
            topic_matches(filter, topic),
            expected,
            "filter {filter:?} against topic {topic:?}"
        );
    }
}

#[test]
fn topic_names_must_be_non_empty_and_free_of_wildcards_and_nul() {
    let valid = [
        "home/loft/temp",
        "a",
        "/",
        "/a/",
        "a//b",
        " ",
        "$SYS/x",
        "é/€",
    ];
    let invalid = [
        "",
        "+",
        "#",
        "home/+/temp",
        "home/#",
        "a+b",
        "a#",
        "\0",
        "home/\0/x",
    ];
    for name in valid {
        assert!(valid_topic_name(name), "{name:?} is a topic name");
    }
    for name in invalid {
        assert!(!valid_topic_name(name), "{name:?} is not a topic name");
    }
}

#[test]
fn topic_filters_must_keep_each_wildcard_on_its_own_level() {
    let valid = [
        "#",
        "+",
        "sport/#",
        "sport/+/player1",
        "+/+/+",
        "+/#",
        "/finance",
        "/",
        "/#",
        "/+",
        "a/+/",
        "a//b",
        "a b",
        "home/loft/temp",
        "$SYS/#",
        "$share/g/a/#",
    ];
    let invalid = [
        "",
        "\0",
        "a\0",
        "sport/#/ranking",
        "sport#",
        "#/a",
        "#/",
        "a/#/",
        "##",
        "a/##",
        "sport+",
        "+sport",
        "sp+rt/x",
        "a/b+/c",
        "++",
        "a/+#",
        "a/#+",
    ];
    for filter in valid {
        assert!(valid_topic_filter(filter), "{filter:?} is a topic filter");
    }
    for filter in invalid {
        assert!(
            !valid_topic_filter(filter),
            "{filter:?} is not a topic filter"
        );
    }
}

#[test]
fn shared_subscription_filters_split_into_group_and_filter() {
    let shared = |group: &str, filter: &str| {
        Some(SharedFilter {
            group: group.to_owned(),
            filter: filter.to_owned(),
        })
    };
    let cases = [
        (
            "$share/consumers/sport/tennis/+",
            shared("consumers", "sport/tennis/+"),
        ),
        ("$share/g/#", shared("g", "#")),
        ("$share/g/+", shared("g", "+")),
        ("$share/g//x", shared("g", "/x")),
        ("$share/a b/x", shared("a b", "x")),
        // Not a shared subscription at all.
        ("sport/#", None),
        ("share/g/x", None),
        ("/$share/g/x", None),
        ("$SHARE/g/x", None),
        ("$share", None),
        // §4.8.2: the ShareName is at least one character and has no `/`,
        // `+` or `#`; a topic filter must follow it.
        ("$share/", None),
        ("$share//x", None),
        ("$share/grp", None),
        ("$share/grp/", None),
        ("$share/g+/x", None),
        ("$share/g#/x", None),
        ("$share/+/x", None),
    ];
    for (filter, expected) in cases {
        assert_eq!(parse_shared(filter), expected, "{filter:?}");
    }
}

// ---- authentication -----------------------------------------------------------------

fn authenticator() -> Authenticator {
    let mut auth = Authenticator::default();
    auth.add_user("admin", b"s3cret");
    auth.add_user("empty", b"");
    auth
}

#[test]
fn a_fresh_authenticator_refuses_everything() {
    let auth = Authenticator::default();
    assert!(!auth.authenticate(None, None));
    assert!(!auth.authenticate(Some("admin"), Some(b"s3cret")));
    assert!(!auth.authorize(None, AclAction::Publish, "a"));
    assert!(!auth.authorize(Some("admin"), AclAction::Subscribe, "a"));
}

/// `(user name, password, accepted)`.
type Login = (Option<&'static str>, Option<&'static [u8]>, bool);

#[test]
fn credentials_must_match_a_known_user_exactly() {
    let auth = authenticator();
    let cases: [Login; 12] = [
        (Some("admin"), Some(b"s3cret"), true),
        (Some("admin"), Some(b"wrong"), false),
        (Some("admin"), Some(b"s3cret "), false),
        (Some("admin"), Some(b""), false),
        (Some("admin"), None, false),
        (Some("ADMIN"), Some(b"s3cret"), false),
        (Some("ghost"), Some(b"s3cret"), false),
        (Some("ghost"), None, false),
        (Some(""), Some(b"s3cret"), false),
        // A stored empty password needs an empty password, not a missing one.
        (Some("empty"), Some(b""), true),
        (Some("empty"), None, false),
        // No user name is anonymous, and anonymous access is off.
        (None, None, false),
    ];
    for (username, password, expected) in cases {
        assert_eq!(
            auth.authenticate(username, password),
            expected,
            "user {username:?} password {password:?}"
        );
    }
    assert!(!auth.authenticate(None, Some(b"s3cret")));
}

#[test]
fn anonymous_access_applies_only_without_a_user_name() {
    let mut auth = authenticator();
    auth.set_anonymous(true);
    assert!(auth.authenticate(None, None));
    // The password of an anonymous client is not looked at.
    assert!(auth.authenticate(None, Some(b"anything")));
    assert!(!auth.authenticate(Some("ghost"), None));
    assert!(!auth.authenticate(Some("admin"), Some(b"wrong")));
    assert!(auth.authenticate(Some("admin"), Some(b"s3cret")));
    auth.set_anonymous(false);
    assert!(!auth.authenticate(None, None));
}

#[test]
fn adding_a_user_again_replaces_the_password() {
    let mut auth = authenticator();
    auth.add_user("admin", b"new");
    assert!(!auth.authenticate(Some("admin"), Some(b"s3cret")));
    assert!(auth.authenticate(Some("admin"), Some(b"new")));
}

// ---- topic ACL ------------------------------------------------------------------------

fn acl() -> Authenticator {
    let mut auth = Authenticator::default();
    auth.deny("admin", AclAction::Publish, "home/secret/#");
    auth.allow("admin", AclAction::Publish, "home/#");
    auth.allow_any(AclAction::Subscribe, "public/#");
    auth.deny("guest", AclAction::Subscribe, "#");
    auth.allow("guest", AclAction::Publish, "+/guest");
    auth
}

const PUBLISH: AclAction = AclAction::Publish;
const SUBSCRIBE: AclAction = AclAction::Subscribe;

/// `(user, action, topic, verdict with default deny, verdict with default
/// allow)` against the rules of `acl`. The two verdicts agree exactly when
/// a rule decided.
const ACL_VERDICTS: &[(Option<&str>, AclAction, &str, bool, bool)] = &[
    // The first matching rule decides.
    (Some("admin"), PUBLISH, "home/secret/code", false, false),
    (Some("admin"), PUBLISH, "home/secret", false, false),
    (Some("admin"), PUBLISH, "home/loft/temp", true, true),
    (Some("admin"), PUBLISH, "home", true, true),
    // A rule is for one action only.
    (Some("admin"), SUBSCRIBE, "home/loft/temp", false, true),
    (None, PUBLISH, "public/news", false, true),
    // A rule without a user name is for everybody, anonymous included.
    (Some("admin"), SUBSCRIBE, "public/news", true, true),
    (None, SUBSCRIBE, "public/news", true, true),
    // A rule with a user name is for that user only.
    (None, PUBLISH, "home/loft/temp", false, true),
    (Some("other"), PUBLISH, "home/loft/temp", false, true),
    (Some("Admin"), PUBLISH, "home/loft/temp", false, true),
    // The any-user allow is listed before guest's deny.
    (Some("guest"), SUBSCRIBE, "public/news", true, true),
    (Some("guest"), SUBSCRIBE, "private/news", false, false),
    (Some("guest"), PUBLISH, "x/guest", true, true),
    (Some("guest"), PUBLISH, "x/y/guest", false, true),
    // §4.7.2 also holds for rule filters.
    (Some("guest"), PUBLISH, "$SYS/guest", false, true),
    // A subscription's topic filter is matched level by level like a name.
    (Some("admin"), SUBSCRIBE, "public/#", true, true),
    (Some("admin"), SUBSCRIBE, "public/+/x", true, true),
    (Some("admin"), SUBSCRIBE, "#", false, true),
    (Some("admin"), SUBSCRIBE, "+/news", false, true),
];

#[test]
fn acl_verdicts_follow_rule_order_user_action_and_filter() {
    let auth = acl();
    for &(user, action, topic, expected, _) in ACL_VERDICTS {
        assert_eq!(
            auth.authorize(user, action, topic),
            expected,
            "{user:?} {action:?} {topic:?}"
        );
    }
}

#[test]
fn the_default_verdict_applies_only_when_no_rule_matches() {
    let mut auth = acl();
    auth.set_default_allow(true);
    for &(user, action, topic, _, expected) in ACL_VERDICTS {
        assert_eq!(
            auth.authorize(user, action, topic),
            expected,
            "{user:?} {action:?} {topic:?} with default allow"
        );
    }
    auth.set_default_allow(false);
    assert!(!auth.authorize(Some("other"), PUBLISH, "x"));
}

// ---- bridge topic rules ---------------------------------------------------------------

fn rule(pattern: &str, direction: Direction, qos: QoS, prefixes: (&str, &str)) -> TopicRule {
    let (local_prefix, remote_prefix) = prefixes;
    TopicRule {
        pattern: pattern.to_owned(),
        direction,
        qos,
        local_prefix: local_prefix.to_owned(),
        remote_prefix: remote_prefix.to_owned(),
    }
}

#[test]
fn topic_rule_lines_parse_with_defaults_for_missing_fields() {
    let none = ("", "");
    let cases = [
        (
            "sensors/# out 0 home/ cloud/",
            rule(
                "sensors/#",
                Direction::Out,
                QoS::AtMostOnce,
                ("home/", "cloud/"),
            ),
        ),
        (
            "alerts/+",
            rule("alerts/+", Direction::Both, QoS::AtMostOnce, none),
        ),
        ("a in", rule("a", Direction::In, QoS::AtMostOnce, none)),
        (
            "a both 2",
            rule("a", Direction::Both, QoS::ExactlyOnce, none),
        ),
        (
            "a out 1 lp/",
            rule("a", Direction::Out, QoS::AtLeastOnce, ("lp/", "")),
        ),
        // Any run of whitespace separates fields; fields after the remote
        // prefix are ignored.
        (
            "  a \t out   1   l/   r/   ",
            rule("a", Direction::Out, QoS::AtLeastOnce, ("l/", "r/")),
        ),
        (
            "a out 1 l/ r/ extra tokens",
            rule("a", Direction::Out, QoS::AtLeastOnce, ("l/", "r/")),
        ),
        // The QoS is parsed as an unsigned decimal number.
        (
            "a out +1",
            rule("a", Direction::Out, QoS::AtLeastOnce, none),
        ),
        (
            "a out 02",
            rule("a", Direction::Out, QoS::ExactlyOnce, none),
        ),
    ];
    for (line, expected) in cases {
        assert_eq!(TopicRule::parse(line), Some(expected), "{line:?}");
    }
}

#[test]
fn topic_rule_lines_with_a_bad_direction_or_qos_do_not_parse() {
    let lines = [
        "",
        "   ",
        "a sideways 0",
        "a OUT 0",
        "a 1",
        "a out 3",
        "a out 256",
        "a out -1",
        "a out 1.0",
        "a out x",
        "a out one l/ r/",
    ];
    for line in lines {
        assert_eq!(TopicRule::parse(line), None, "{line:?}");
    }
}

fn bridge(lines: &[&str]) -> Result<BridgeConfig, String> {
    let rules = lines
        .iter()
        .map(|line| TopicRule::parse(line).ok_or_else(|| format!("rule {line:?} did not parse")))
        .collect::<Result<Vec<_>, _>>()?;
    Ok(BridgeConfig {
        name: "cloud".to_owned(),
        remote_addr: "mqtt.example:8883".to_owned(),
        client_id: "cave-bridge".to_owned(),
        remote_username: None,
        remote_password: None,
        rules,
    })
}

fn cloud_bridge() -> Result<BridgeConfig, String> {
    bridge(&[
        "sensors/# out 1 home/ cloud/",
        "commands/# in 1 home/ cloud/",
        "status/+ both 0",
    ])
}

#[test]
fn bridge_subscriptions_follow_rule_order_and_direction() -> TestResult {
    let config = cloud_bridge()?;
    assert_eq!(config.local_subscriptions(), ["home/sensors/#", "status/+"]);
    assert_eq!(
        config.remote_subscriptions(),
        ["cloud/commands/#", "status/+"]
    );
    let none = bridge(&[])?;
    assert_eq!(none.local_subscriptions(), [""; 0]);
    assert_eq!(none.remote_subscriptions(), [""; 0]);
    Ok(())
}

#[test]
fn bridge_maps_topics_by_swapping_the_prefix_of_the_first_matching_rule() -> TestResult {
    let config = cloud_bridge()?;
    let outbound = [
        ("home/sensors/loft/temp", Some("cloud/sensors/loft/temp")),
        ("home/sensors", Some("cloud/sensors")),
        ("status/online", Some("status/online")),
        // Inbound-only rule, too many levels, missing prefix, remote name.
        ("home/commands/x", None),
        ("status/a/b", None),
        ("sensors/x", None),
        ("cloud/sensors/x", None),
    ];
    for (local, remote) in outbound {
        assert_eq!(
            config.map_local_to_remote(local).as_deref(),
            remote,
            "local {local:?}"
        );
    }
    let inbound = [
        ("cloud/commands/reboot", Some("home/commands/reboot")),
        ("cloud/commands", Some("home/commands")),
        ("status/online", Some("status/online")),
        ("cloud/sensors/x", None),
        ("home/commands/x", None),
        ("commands/x", None),
    ];
    for (remote, local) in inbound {
        assert_eq!(
            config.map_remote_to_local(remote).as_deref(),
            local,
            "remote {remote:?}"
        );
    }

    let twice = bridge(&["a/# out 0 l/ r1/", "a/# both 0 l/ r2/"])?;
    assert_eq!(
        twice.map_local_to_remote("l/a/x").as_deref(),
        Some("r1/a/x")
    );
    assert_eq!(
        twice.map_remote_to_local("r2/a/x").as_deref(),
        Some("l/a/x")
    );
    assert_eq!(twice.map_remote_to_local("r1/a/x"), None);
    Ok(())
}

#[test]
fn bridge_mapping_stops_at_a_matching_rule_whose_prefix_cannot_be_stripped() -> TestResult {
    // "home" matches the first rule's local filter "home/#" (the parent
    // level) but does not start with its prefix "home/": the lookup ends
    // there, although the second rule would map the topic.
    let config = bridge(&["# out 0 home/ cloud/", "home out 0"])?;
    assert_eq!(config.local_subscriptions(), ["home/#", "home"]);
    assert_eq!(config.map_local_to_remote("home"), None);
    assert_eq!(
        config.map_local_to_remote("home/x").as_deref(),
        Some("cloud/x")
    );

    let config = bridge(&["# in 0 home/ cloud/", "cloud in 0"])?;
    assert_eq!(config.map_remote_to_local("cloud"), None);
    assert_eq!(
        config.map_remote_to_local("cloud/x").as_deref(),
        Some("home/x")
    );
    Ok(())
}
