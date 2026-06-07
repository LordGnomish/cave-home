//! MQTT topic names, topic filters and wildcard matching (§4.7) plus
//! shared-subscription filter parsing (§4.8.2). Clean-room from spec.

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn topic_names_reject_wildcards_and_empty() {
        // §4.7.3: a Topic Name must not contain '+' or '#' and must be
        // at least one character, with no U+0000.
        assert!(valid_topic_name("home/loft/temp"));
        assert!(valid_topic_name("/"));
        assert!(!valid_topic_name(""));
        assert!(!valid_topic_name("home/+/temp"));
        assert!(!valid_topic_name("home/#"));
        assert!(!valid_topic_name("home/\0/x"));
    }

    #[test]
    fn topic_filters_enforce_wildcard_placement() {
        // §4.7.1.2 / §4.7.1.3.
        assert!(valid_topic_filter("#"));
        assert!(valid_topic_filter("+"));
        assert!(valid_topic_filter("sport/#"));
        assert!(valid_topic_filter("sport/+/player1"));
        assert!(valid_topic_filter("+/+/+"));
        assert!(valid_topic_filter("/finance"));
        // '#' must be the last character and occupy its own level.
        assert!(!valid_topic_filter("sport/#/ranking"));
        assert!(!valid_topic_filter("sport#"));
        // '+' must occupy an entire level.
        assert!(!valid_topic_filter("sport+"));
        assert!(!valid_topic_filter("sp+rt/x"));
        assert!(!valid_topic_filter(""));
    }

    #[test]
    fn multi_level_wildcard_matches_parent_and_descendants() {
        // §4.7.1.2 examples.
        assert!(topic_matches("sport/tennis/player1/#", "sport/tennis/player1"));
        assert!(topic_matches("sport/tennis/player1/#", "sport/tennis/player1/ranking"));
        assert!(topic_matches("sport/tennis/player1/#", "sport/tennis/player1/score/wimbledon"));
        assert!(topic_matches("sport/#", "sport"));
        assert!(topic_matches("#", "anything/at/all"));
    }

    #[test]
    fn single_level_wildcard_matches_exactly_one_level() {
        // §4.7.1.3 examples.
        assert!(topic_matches("sport/tennis/+", "sport/tennis/player1"));
        assert!(!topic_matches("sport/tennis/+", "sport/tennis/player1/ranking"));
        assert!(!topic_matches("sport/+", "sport"));
        assert!(topic_matches("sport/+", "sport/"));
        assert!(topic_matches("+/+", "/finance"));
        assert!(topic_matches("/+", "/finance"));
        assert!(!topic_matches("+", "/finance"));
    }

    #[test]
    fn dollar_topics_are_shielded_from_leading_wildcards() {
        // §4.7.2: wildcards at the top level do not match $-topics.
        assert!(!topic_matches("#", "$SYS/broker/clients"));
        assert!(!topic_matches("+/monitor/Clients", "$SYS/monitor/Clients"));
        assert!(topic_matches("$SYS/#", "$SYS/broker/clients"));
        assert!(topic_matches("$SYS/monitor/+", "$SYS/monitor/Clients"));
    }

    #[test]
    fn exact_match_without_wildcards() {
        assert!(topic_matches("a/b/c", "a/b/c"));
        assert!(!topic_matches("a/b/c", "a/b/d"));
        assert!(!topic_matches("a/b", "a/b/c"));
    }

    #[test]
    fn shared_subscription_filter_parsing() {
        // §4.8.2: $share/{ShareName}/{filter}.
        let s = parse_shared("$share/consumers/sport/tennis/+").expect("shared");
        assert_eq!(s.group, "consumers");
        assert_eq!(s.filter, "sport/tennis/+");
        assert!(parse_shared("sport/#").is_none());
        // ShareName must be non-empty and contain no wildcards or '/'.
        assert!(parse_shared("$share//x").is_none());
        assert!(parse_shared("$share/grp").is_none());
    }
}
