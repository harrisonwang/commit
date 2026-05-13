use commit_core::{config, message, message::FooterSeparator};

fn default_config() -> config::Config {
    config::Config::default_with_home(tempfile::tempdir().expect("home").path().to_path_buf())
}

#[test]
fn parses_official_spec_examples() {
    let breaking_footer = message::parse(
        "feat: allow provided config object to extend other configs\n\nBREAKING CHANGE: `extends` key in config file is now used for extending other config files",
    )
    .expect("parse breaking footer example");
    assert_eq!(breaking_footer.header.ty, "feat");
    assert_eq!(breaking_footer.body, None);
    assert!(breaking_footer.breaking);
    assert_eq!(breaking_footer.footers.len(), 1);
    assert_eq!(breaking_footer.footers[0].token, "BREAKING CHANGE");

    let bang = message::parse("feat!: send an email to the customer when a product is shipped")
        .expect("parse breaking bang example");
    assert!(bang.header.breaking);
    assert_eq!(
        bang.breaking_description,
        Some("send an email to the customer when a product is shipped")
    );

    let scoped_bang =
        message::parse("feat(api)!: send an email to the customer when a product is shipped")
            .expect("parse scoped breaking bang example");
    assert_eq!(scoped_bang.header.scope, Some("api"));
    assert!(scoped_bang.breaking);

    let bang_and_footer = message::parse(
        "feat!: drop support for Node 6\n\nBREAKING CHANGE: use JavaScript features not available in Node 6.",
    )
    .expect("parse bang plus footer example");
    assert_eq!(
        bang_and_footer.breaking_description,
        Some("use JavaScript features not available in Node 6.")
    );

    let docs = message::parse("docs: correct spelling of CHANGELOG").expect("parse docs example");
    assert_eq!(docs.header.ty, "docs");

    let scoped = message::parse("feat(lang): add polish language").expect("parse scope example");
    assert_eq!(scoped.header.scope, Some("lang"));

    let body_and_footers = message::parse(
        "fix: prevent racing of requests\n\nIntroduce a request id and a reference to latest request. Dismiss\nincoming responses other than from latest request.\n\nRemove timeouts which were used to mitigate the racing issue but are\nobsolete now.\n\nReviewed-by: Z\nRefs: #123",
    )
    .expect("parse body and footers example");
    assert!(
        body_and_footers
            .body
            .expect("body")
            .contains("Remove timeouts")
    );
    assert_eq!(body_and_footers.footers.len(), 2);
    assert_eq!(body_and_footers.footers[0].token, "Reviewed-by");
    assert_eq!(body_and_footers.footers[1].token, "Refs");
    assert_eq!(body_and_footers.footers[1].value, "#123");

    let revert = message::parse(
        "revert: let us never again speak of the noodle incident\n\nRefs: 676104e, a215868",
    )
    .expect("parse revert example");
    assert_eq!(revert.header.ty, "revert");
    assert_eq!(revert.footers[0].value, "676104e, a215868");
}

#[test]
fn lint_accepts_spec_examples_when_types_are_allowed() {
    let mut config = default_config();
    config.allowed_types.push("revert".to_string());

    assert_eq!(
        message::validate("docs: correct spelling of CHANGELOG", &config),
        Ok(())
    );
    assert_eq!(
        message::validate("feat(lang): add polish language", &config),
        Ok(())
    );
    assert_eq!(
        message::validate(
            "revert: let us never again speak of the noodle incident\n\nRefs: 676104e, a215868",
            &config
        ),
        Ok(())
    );
}

#[test]
fn rejects_spec_implied_invalid_headers() {
    for invalid in [
        "feat add user login",
        "feat:add user login",
        ": add user login",
        "feat:",
        "feat: ",
        "feat(scope: update parser",
        "feat)!: update parser",
        "feat(api) !: change API",
        "feat(api):",
        "feat(): add feature",
        "feat lang: add polish language",
        "feat !: remove API",
        "feat(api) !: remove API",
        "feat!: ",
    ] {
        assert!(message::parse(invalid).is_err(), "accepted {invalid:?}");
    }
}

#[test]
fn keeps_malformed_footer_like_text_in_body() {
    for message_text in [
        "feat: add thing\n\nBody text.\n\nReviewed by: Z",
        "feat: add thing\n\nBody text.\n\nReviewed-by:",
        "feat: add thing\n\nBody text.\n\nRefs#123",
        "feat: add thing\n\nBody text.\n\nRefs : #123",
        "feat: remove API\n\nBreaking Change: removed API",
        "feat: remove API\n\nBREAKING CHANGE removed API",
    ] {
        let parsed = message::parse(message_text).expect("parse message with body text");
        assert!(
            parsed.footers.is_empty(),
            "parsed footer in {message_text:?}"
        );
        assert!(!parsed.breaking, "marked breaking in {message_text:?}");
        assert!(parsed.body.is_some(), "missing body in {message_text:?}");
    }
}

#[test]
fn parses_spec_outside_but_structurally_valid_messages() {
    let custom_type = message::parse("release(cli): publish 1.2.3").expect("parse custom type");
    assert_eq!(custom_type.header.ty, "release");
    assert_eq!(custom_type.header.scope, Some("cli"));

    let uppercase_type = message::parse("FEAT: add uppercase type").expect("parse uppercase type");
    assert_eq!(uppercase_type.header.ty, "FEAT");

    let utf8_scope = message::parse("fix(解析器): 处理空输入").expect("parse utf8 scope");
    assert_eq!(utf8_scope.header.scope, Some("解析器"));
    assert_eq!(utf8_scope.header.description, "处理空输入");

    let custom_breaking_like = message::parse("feat: remove API\n\nBREAKINGCHANGE: removed API")
        .expect("parse custom footer token");
    assert_eq!(custom_breaking_like.footers[0].token, "BREAKINGCHANGE");
    assert!(!custom_breaking_like.breaking);

    let token_hash =
        message::parse("chore: update issue\n\nIssue #456").expect("parse hash footer");
    assert_eq!(token_hash.footers[0].token, "Issue");
    assert_eq!(token_hash.footers[0].separator, FooterSeparator::Hash);
    assert_eq!(token_hash.footers[0].value, "#456");
}

#[test]
fn lint_policy_stays_stricter_than_parser() {
    let config = default_config();

    assert_eq!(
        message::validate(
            "revert: let us never again speak of the noodle incident",
            &config
        ),
        Err("commit type 'revert' is not allowed; allowed types: feat, fix, docs, test, ci, build, deps, chore, refactor, perf, style".to_string())
    );
    assert_eq!(
        message::validate("release(cli): publish 1.2.3", &config),
        Err("commit type 'release' is not allowed; allowed types: feat, fix, docs, test, ci, build, deps, chore, refactor, perf, style".to_string())
    );
}
#[test]
fn validates_commit_messages() {
    let config = default_config();
    assert_eq!(message::validate("feat: add example file", &config), Ok(()));
    assert_eq!(
        message::validate("fix(parser): handle empty input", &config),
        Ok(())
    );
    assert_eq!(
        message::validate("", &config),
        Err("generated commit message is empty".to_string())
    );
    assert_eq!(
        message::validate("add example file", &config),
        Err("commit subject must look like '<type>: <summary>'".to_string())
    );
    assert_eq!(
        message::validate("feat: add example file.", &config),
        Err("commit subject must not end with punctuation".to_string())
    );
    assert_eq!(
        message::validate("feat: 提升用户体验", &config),
        Err("commit message contains banned phrase: 提升用户体验".to_string())
    );
}

#[test]
fn parses_basic_headers() {
    let parsed = message::parse("feat(api)!: change public API").expect("parse message");

    assert_eq!(parsed.header.ty, "feat");
    assert_eq!(parsed.header.scope, Some("api"));
    assert!(parsed.header.breaking);
    assert_eq!(parsed.header.description, "change public API");
    assert!(parsed.breaking);
    assert_eq!(parsed.breaking_description, Some("change public API"));

    let trailing = message::parse("docs: update guide\n\n\n").expect("parse trailing newlines");
    assert_eq!(trailing.header.ty, "docs");
    assert_eq!(trailing.body, None);
    assert!(trailing.footers.is_empty());
}

#[test]
fn parses_body() {
    let parsed =
        message::parse("feat: add thing\n\nExplain why.\nMore detail.").expect("parse message");

    assert_eq!(parsed.header.ty, "feat");
    assert_eq!(parsed.body, Some("Explain why.\nMore detail."));
    assert!(parsed.footers.is_empty());
}

#[test]
fn parses_footers_and_breaking_footer() {
    let parsed = message::parse(
        "feat: add thing\n\nBody text.\n\nCo-Authored-By: Lisa Simpson <lisa@example.com>\nCloses #12\nBREAKING CHANGE: old behavior removed",
    )
    .expect("parse message");

    assert_eq!(parsed.body, Some("Body text."));
    assert_eq!(parsed.footers.len(), 3);
    assert_eq!(parsed.footers[0].token, "Co-Authored-By");
    assert_eq!(parsed.footers[0].separator, FooterSeparator::Colon);
    assert_eq!(parsed.footers[0].value, "Lisa Simpson <lisa@example.com>");
    assert_eq!(parsed.footers[1].token, "Closes");
    assert_eq!(parsed.footers[1].separator, FooterSeparator::Hash);
    assert_eq!(parsed.footers[1].value, "#12");
    assert!(parsed.breaking);
    assert_eq!(parsed.breaking_description, Some("old behavior removed"));
}

#[test]
fn parses_footer_only_and_multiline_footer_values() {
    let breaking = message::parse("feat: change API\n\nBREAKING CHANGE: old behavior removed")
        .expect("parse breaking footer");
    assert_eq!(breaking.body, None);
    assert!(breaking.breaking);
    assert_eq!(breaking.breaking_description, Some("old behavior removed"));

    let multiline = message::parse(
        "fix: update release notes\n\nRefs: first line\nsecond line\n\nReviewed-by: Lisa Simpson <lisa@example.com>",
    )
    .expect("parse multiline footer");
    assert_eq!(multiline.body, None);
    assert_eq!(multiline.footers.len(), 2);
    assert_eq!(multiline.footers[0].token, "Refs");
    assert_eq!(multiline.footers[0].value, "first line\nsecond line");
    assert_eq!(multiline.footers[1].token, "Reviewed-by");
}

#[test]
fn parses_footer_blocks_separated_by_blank_lines() {
    let parsed = message::parse(
        "feat: thing\n\nbody\n\nCloses #123\n\nBREAKING CHANGE: broke\n\nCo-Authored-By: Lisa <lisa@example.com>",
    )
    .expect("parse separated footers");

    assert_eq!(parsed.body, Some("body"));
    assert_eq!(parsed.footers.len(), 3);
    assert_eq!(parsed.footers[0].token, "Closes");
    assert_eq!(parsed.footers[1].token, "BREAKING CHANGE");
    assert_eq!(parsed.footers[2].token, "Co-Authored-By");
    assert_eq!(parsed.breaking_description, Some("broke"));
}

#[test]
fn parses_crlf_and_keeps_fake_footer_in_body() {
    let crlf =
        message::parse("feat: thing\r\n\r\nbody\r\n\r\nCloses #123\r\nBREAKING-CHANGE: broke\r\n")
            .expect("parse crlf message");
    assert_eq!(crlf.body, Some("body"));
    assert_eq!(crlf.footers.len(), 2);
    assert_eq!(crlf.breaking_description, Some("broke"));

    let fake_footer = message::parse(
        "fix: something\n\nFirst line of body\nIMPORTANT: this is not a footer\nAnother line",
    )
    .expect("parse body");
    assert!(fake_footer.footers.is_empty());
    assert_eq!(
        fake_footer.body,
        Some("First line of body\nIMPORTANT: this is not a footer\nAnother line")
    );
}

#[test]
fn rejects_invalid_headers() {
    assert!(message::parse("feat(): empty scope").is_err());
    assert!(message::parse("feat!(api): misplaced bang").is_err());
    assert!(message::parse("feat:").is_err());
    assert!(message::parse("feat: ").is_err());
    assert!(message::parse("docs:提交文件").is_err());
    assert!(message::parse(" feat: leading whitespace").is_err());
    assert!(message::parse("chore: title\nchanged without blank line").is_err());
}

#[test]
fn rejects_unclosed_parentheses_in_header_prefix() {
    for invalid in [
        "feat(api: change API",
        "feat(api(scope): change API",
        "feat)api(: change API",
    ] {
        assert!(message::parse(invalid).is_err(), "accepted {invalid:?}");
    }
}

#[test]
fn parses_freeform_multiline_footer_values_after_body() {
    let parsed = message::parse(
        "feat: add thing\n\nBody text.\n\nReviewed-by: Z\nfreeform continuation line",
    )
    .expect("parse freeform multiline footer");

    assert_eq!(parsed.body, Some("Body text."));
    assert_eq!(parsed.footers.len(), 1);
    assert_eq!(parsed.footers[0].value, "Z\nfreeform continuation line");
}

#[test]
fn preserves_body_blank_lines_before_footers() {
    let parsed =
        message::parse("feat: add thing\n\nFirst paragraph.\n\nSecond paragraph.\n\nRefs: #123")
            .expect("parse body paragraphs and footer");

    assert_eq!(parsed.body, Some("First paragraph.\n\nSecond paragraph."));
    assert_eq!(parsed.footers.len(), 1);
    assert_eq!(parsed.footers[0].token, "Refs");
}

#[test]
fn parses_multiline_footer_value_with_blank_line() {
    let parsed = message::parse("fix: update notes\n\nRefs: first paragraph\n\nsecond paragraph")
        .expect("parse multiline footer with blank line");

    assert_eq!(parsed.body, None);
    assert_eq!(parsed.footers.len(), 1);
    assert_eq!(
        parsed.footers[0].value,
        "first paragraph\n\nsecond paragraph"
    );
}

#[test]
fn rejects_empty_footer_values_in_footer_only_messages() {
    for invalid in ["feat: add thing\n\nRefs: ", "feat: add thing\n\nRefs #"] {
        assert!(message::parse(invalid).is_err(), "accepted {invalid:?}");
    }
}

#[test]
fn parses_lowercase_breaking_change_as_normal_footer() {
    let parsed = message::parse("feat: remove API\n\nbreaking-change: removed API")
        .expect("parse lowercase custom footer");

    assert_eq!(parsed.footers.len(), 1);
    assert_eq!(parsed.footers[0].token, "breaking-change");
    assert!(!parsed.breaking);
}

#[test]
fn lint_uses_raw_header_length_without_trimming() {
    let mut config = default_config();
    config.max_subject_chars = 16;

    assert_eq!(
        message::validate("feat: add file   ", &config),
        Err("commit subject is longer than 16 characters".to_string())
    );
}

#[test]
fn parses_case_insensitive_allowed_types() {
    let mut config = default_config();
    config.allowed_types = vec!["feat".to_string()];

    assert_eq!(
        message::validate("FEAT: add uppercase type", &config),
        Ok(())
    );
}

#[test]
fn configurable_allowed_types_are_lint_policy() {
    let config = default_config();

    assert!(message::parse("deps: update package").is_ok());
    assert_eq!(message::validate("deps: update package", &config), Ok(()));
}

#[test]
fn custom_allowed_types_can_exclude_default_deps_type() {
    let mut config = default_config();
    config.allowed_types = vec!["feat".to_string()];

    assert!(message::parse("deps: update package").is_ok());
    assert_eq!(
        message::validate("deps: update package", &config),
        Err("commit type 'deps' is not allowed; allowed types: feat".to_string())
    );
}
