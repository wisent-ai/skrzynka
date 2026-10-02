mod support;

use anyhow::{ensure, Context, Result};
use support::{expected_body, required, Run};

#[test]
fn imports_and_reads_real_mail_without_duplicate_rows_or_cross_organization_reads() {
    let mut run = Run::new().expect("create the retained real-test report directory");
    let outcome = exercise(&mut run);
    let cleanup = run.clean();
    run.finish(&outcome, &cleanup)
        .expect("retain the real-test report and remove isolated local state");
    assert!(
        outcome.is_ok() && cleanup.is_ok(),
        "mailbox qualification: {outcome:?}; cleanup: {cleanup:?}"
    );
}

fn exercise(run: &mut Run) -> Result<()> {
    let item = required("SKRZYNKA_TEST_ITEM")?;
    let message_id = required("SKRZYNKA_TEST_MESSAGE_ID")?;
    let subject = required("SKRZYNKA_TEST_SUBJECT")?;
    let email = required("SKRZYNKA_TEST_EMAIL")?;
    let body = expected_body()?;
    run.record_fixture(serde_json::json!({
        "item": item, "message_id": message_id, "email": email,
        "subject": subject, "body": body,
    }));
    ensure!(
        !body.trim().is_empty(),
        "the known provider fixture must have a nonempty plain-text body"
    );
    run.prepare(&item)?;
    let version = run.ok(&["version"])?;
    ensure!(
        version["source"] == run.revision,
        "compile with SKRZYNKA_SOURCE_REVISION set to the checkout revision being qualified"
    );
    let initial = run.ok(&["status"])?;
    ensure!(
        initial["mailbox_count"] == 0 && initial["message_count"] == 0,
        "test organization was not empty"
    );

    let imported = run.declare()?;
    ensure!(
        imported["applied"] == true,
        "mailbox declaration was not applied: {imported}"
    );
    ensure!(
        imported["has_more"] == false,
        "use a dedicated provider fixture that fits one bounded import page"
    );
    let mailbox = &imported["mailbox"];
    let mailbox_id = mailbox["id"]
        .as_str()
        .context("declaration omitted mailbox id")?
        .to_owned();
    ensure!(
        mailbox["email"] == email && mailbox["skarbiec_item_id"] == item,
        "import selected a different mailbox"
    );
    let cursor = mailbox["last_uid"]
        .as_u64()
        .context("declaration omitted provider cursor")?;
    let messages = run.ok(&[
        "message",
        "list",
        "--mailbox",
        &mailbox_id,
        "--limit",
        "500",
    ])?;
    let matches: Vec<_> = messages
        .as_array()
        .context("message list did not return an array")?
        .iter()
        .filter(|message| message["message_id"] == message_id)
        .collect();
    ensure!(
        matches.len() == 1,
        "expected exactly one imported copy of the known provider message, found {}",
        matches.len()
    );
    let id = matches[0]["id"]
        .as_str()
        .context("imported message omitted id")?;
    let stored = run.ok(&["message", "show", id])?;
    ensure!(
        stored["mailbox_id"] == mailbox_id && stored["message_id"] == message_id,
        "persisted message identity changed"
    );
    ensure!(
        stored["subject"] == subject && stored["body_text"] == body,
        "persisted subject or full message body differs from the independent provider fixture"
    );
    let uid = stored["external_uid"]
        .as_u64()
        .context("message omitted provider UID")?;
    ensure!(
        uid > 0 && cursor >= uid,
        "the committed cursor does not cover the imported provider message"
    );

    let synchronized = run.ok(&["sync", "--mailbox", &mailbox_id])?;
    ensure!(
        synchronized["received"] == 0 && synchronized["last_uid"] == cursor,
        "the unchanged fixture was imported twice or its cursor changed"
    );
    ensure!(
        run.ok(&[
            "message",
            "list",
            "--mailbox",
            &mailbox_id,
            "--limit",
            "500"
        ])? == messages,
        "a second provider pass changed the persisted message set"
    );
    ensure!(
        run.ok(&["message", "show", id])? == stored,
        "a fresh CLI process read different persisted message content"
    );

    let own_organization = std::mem::replace(
        &mut run.organization,
        format!("mailbox-test-{}", uuid::Uuid::new_v4()),
    );
    let outside = run.refusal(&["message", "show", id], "NOT_FOUND");
    run.organization = own_organization;
    outside?;
    run.refusal(&["mailbox", "remove", &mailbox_id], "CONFIRMATION_REQUIRED")?;
    run.refusal(
        &["mailbox", "remove", &mailbox_id, "--confirm"],
        "MAILBOX_STILL_DECLARED",
    )?;
    ensure!(
        run.ok(&["message", "show", id])? == stored,
        "a refused removal changed the stored message"
    );
    let disabled = run.ok(&["mailbox", "undeclare", &mailbox_id])?;
    ensure!(
        disabled["enabled"] == false,
        "undeclared mailbox remains enabled"
    );
    ensure!(
        run.ok(&["message", "show", id])? == stored,
        "undeclaring removed readable mail"
    );
    run.ok(&["mailbox", "remove", &mailbox_id, "--confirm"])?;
    run.refusal(&["mailbox", "show", &mailbox_id], "NOT_FOUND")?;
    run.refusal(&["message", "show", id], "NOT_FOUND")?;
    Ok(())
}
