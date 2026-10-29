//! The card writer (#328, Testing Decisions): the real `claude -p` command is checked without
//! running it, and the cards come from its reply.

use std::{ffi::OsStr, path::Path};
use studio_core::{CardRequest, Step};
use studio_understanding::{Card, ClaudeWriter};

fn edit() -> CardRequest {
    CardRequest {
        session_id: "2c7a9f14-6e3b-4d58-a1c9-0f8e2b7d4a63".into(),
        tool_use_id: "toolu_01Hc4Xe7Lm2Qp9Rs5Tv8Wy3Z".into(),
        step: Step {
            tool: "Edit".into(),
            detail: Some("apps/studio/ui/style.css".into()),
        },
        change: concat!(
            "@@ -9,1 +9,1 @@\n",
            "-.unchecked { font-size:12px; }\n",
            "+.unchecked { font-size:15px; }\n",
        )
        .into(),
    }
}

#[test]
fn the_real_writer_runs_claude_on_the_subscription_in_its_neutral_folder() {
    let folder = Path::new("/h/Library/Application Support/dev.falinks.studio/card-writer");
    let command = ClaudeWriter::new(folder.into()).command(&edit());

    assert_eq!(command.get_program(), "claude");
    // Both variables would take precedence over the subscription login; nothing else is passed.
    let mut env: Vec<_> = command.get_envs().collect();
    env.sort();
    assert_eq!(
        env,
        [
            (OsStr::new("ANTHROPIC_API_KEY"), None),
            (OsStr::new("ANTHROPIC_AUTH_TOKEN"), None),
        ]
    );
    let args: Vec<_> = command.get_args().map(|arg| arg.to_str().unwrap()).collect();
    assert_eq!(args[0], "-p");
    assert!(!args.contains(&"--bare"));
    let prompt = args[1];
    assert!(prompt.contains("why the agent took this step"));
    assert!(prompt.contains("Edit apps/studio/ui/style.css"));
    assert!(prompt.contains(&edit().change));
    assert_eq!(command.get_current_dir(), Some(folder));
}

#[test]
fn a_reply_with_one_json_object_gives_the_card_even_in_a_code_fence() {
    let reply = concat!(
        "```json\n",
        r#"{"question": "למה הסוכן הגדיל את `font-size` של `.unchecked`?", "#,
        r#""answer": "כדי שאפשר יהיה לקרוא את שורת „לא נבדק” בלי מאמץ.", "#,
        r#""paths": ["apps/studio/ui/style.css"], "#,
        r#""code": "-.unchecked { font-size:12px; }\n+.unchecked { font-size:15px; }", "#,
        r#""why_it_matters": "טקסט משני ב־`12px` היה קטן מדי לקריאה בזמן המתנה.", "#,
        r#""not_checked": "לא נבדק אם הכרטיס עדיין נכנס בחלונית צרה."}"#,
        "\n```\n",
    );

    assert_eq!(
        Card::from_reply(reply),
        Ok(Card {
            question: "למה הסוכן הגדיל את `font-size` של `.unchecked`?".into(),
            answer: "כדי שאפשר יהיה לקרוא את שורת „לא נבדק” בלי מאמץ.".into(),
            paths: vec!["apps/studio/ui/style.css".into()],
            code: "-.unchecked { font-size:12px; }\n+.unchecked { font-size:15px; }".into(),
            why_it_matters: "טקסט משני ב־`12px` היה קטן מדי לקריאה בזמן המתנה.".into(),
            not_checked: "לא נבדק אם הכרטיס עדיין נכנס בחלונית צרה.".into(),
        })
    );
}

#[test]
fn a_reply_that_does_not_parse_gives_no_card_and_one_log_line() {
    let replies = [
        "",
        "I can't write a card without the file.\nShall I read it?",
        r#"{"question": "למה?", "why_it_matters": "כי."}"#,
    ];

    for reply in replies {
        let line = Card::from_reply(reply).expect_err(reply);
        assert_eq!(line.lines().count(), 1, "{line}");
    }
}
