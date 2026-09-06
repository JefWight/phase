//! Regression: "Target artifact you control becomes a copy of a second target
//! artifact you control" must copy onto the ANNOUNCED RECIPIENT, not onto the
//! ability's source.
//!
//! Shuri, Wakandan Inventor:
//!   "{1}, {T}: Target artifact you control becomes a copy of a second target
//!    artifact you control until end of turn, except it isn't legendary.
//!    Activate only as a sorcery."
//!
//! Reported behaviour: activating Shuri prompted for only ONE target and turned
//! SHURI HERSELF into the copy. Cause: the singular "becomes a copy of" parser
//! arm hardcoded the recipient to the ability source and discarded the parsed
//! subject entirely, so the printed "Target artifact you control" recipient was
//! never represented and never announced.
//!
//! This is a class defect, not a Shuri defect — every card whose copy recipient
//! is not its own source was affected (True Polymorph, Shapesharer, Saheeli
//! Sublime Artificer, The Animus, Reflection Net, and the untargeted-mass forms
//! Mirrorweave / Mirrorform). The fix introduces the typed `CopyRecipient` axis;
//! these tests pin both the parsed shape and the runtime behaviour.
//!
//! CR references (verified against `docs/MagicCompRules.txt`):
//!   - CR 115.1: targets are declared as the ability is put on the stack.
//!   - CR 115.3: each instance of the word "target" is a separate instance; the
//!     same object may be chosen once for each.
//!   - CR 601.2c: targets are announced in the order written.
//!   - CR 611.2c: an untargeted set affected by a resolution-generated
//!     continuous effect is fixed when the effect begins.
//!   - CR 707.2: a copy acquires the copiable values of the original.

use engine::game::scenario::{GameScenario, P0};
use engine::parser::oracle::parse_oracle_text;
use engine::types::ability::{ControllerRef, CopyRecipient, Effect, TargetFilter, TypeFilter};
use engine::types::mana::{ManaType, ManaUnit};
use engine::types::phase::Phase;
use engine::types::WaitingFor;

const SHURI_ORACLE: &str = concat!(
    "Artifact spells you cast cost {1} less to cast.\n",
    "{1}, {T}: Target artifact you control becomes a copy of a second target artifact ",
    "you control until end of turn, except it isn't legendary. Activate only as a sorcery."
);

/// A donor artifact with a distinctive name so the copy is unmistakable.
const DONOR_ORACLE: &str = "{T}: Add {C}.";

fn shuri_copy_effect() -> Effect {
    let parsed = parse_oracle_text(
        SHURI_ORACLE,
        "Shuri, Wakandan Inventor",
        &[],
        &["Creature".to_string()],
        &[],
    );
    (*parsed
        .abilities
        .iter()
        .find(|a| matches!(&*a.effect, Effect::BecomeCopy { .. }))
        .expect("Shuri must parse a BecomeCopy activated ability")
        .effect)
        .clone()
}

/// SHAPE: CR 115.1 — the printed "Target artifact you control" recipient must
/// survive parsing as an ANNOUNCED recipient, distinct from the copy source.
#[test]
fn shuri_parses_an_announced_artifact_recipient_distinct_from_the_copy_source() {
    let Effect::BecomeCopy {
        target, recipient, ..
    } = shuri_copy_effect()
    else {
        unreachable!("filtered on BecomeCopy above");
    };

    // CR 115.1: the recipient is announced, so it must be `Target(..)` — not
    // `Source` (which silently redirected the copy onto Shuri) and not
    // `Untargeted(..)` (which would skip the target slot).
    let CopyRecipient::Target(recipient_filter) = &recipient else {
        panic!("expected an announced Target recipient, got {recipient:?}");
    };
    let TargetFilter::Typed(tf) = recipient_filter else {
        panic!("expected a typed recipient filter, got {recipient_filter:?}");
    };
    assert!(
        tf.type_filters.contains(&TypeFilter::Artifact),
        "recipient must be restricted to artifacts, got {:?}",
        tf.type_filters
    );
    assert_eq!(
        tf.controller,
        Some(ControllerRef::You),
        "recipient must be restricted to artifacts YOU control"
    );

    // CR 601.2c: the copy source ("a second target artifact you control") is a
    // separate declared target and must still be present.
    let TargetFilter::Typed(source_tf) = &target else {
        panic!("expected a typed copy-source filter, got {target:?}");
    };
    assert!(
        source_tf.type_filters.contains(&TypeFilter::Artifact),
        "copy source must be restricted to artifacts, got {:?}",
        source_tf.type_filters
    );
    assert_eq!(source_tf.controller, Some(ControllerRef::You));
}

/// RUNTIME: CR 707.2 + CR 115.1 — activating Shuri prompts for TWO targets and
/// copies the donor onto the announced recipient, leaving Shuri untouched.
///
/// This is the assertion that flips when the fix is reverted: before it, the
/// ability surfaced one slot and rewrote Shuri's own characteristics.
#[test]
fn shuri_copies_the_donor_onto_the_targeted_recipient_and_not_onto_herself() {
    let mut scenario = GameScenario::new();
    scenario.at_phase(Phase::PreCombatMain);

    let shuri = scenario
        .add_creature_from_oracle(P0, "Shuri, Wakandan Inventor", 2, 1, SHURI_ORACLE)
        .as_legendary()
        .id();
    let recipient = scenario
        .add_artifact_from_oracle(P0, "Recipient Relic", DONOR_ORACLE)
        .id();
    let donor = scenario
        .add_artifact_from_oracle(P0, "Donor Engine", DONOR_ORACLE)
        .id();

    // {1} for the activation cost (the {T} is paid by tapping Shuri).
    scenario.with_mana_pool(
        P0,
        vec![ManaUnit::new(ManaType::Colorless, shuri, false, Vec::new())],
    );

    let mut runner = scenario.build();

    let shuri_name_before = runner.state().objects[&shuri].name.clone();
    let ability_index = runner.state().objects[&shuri]
        .abilities
        .iter()
        .position(|a| matches!(*a.effect, Effect::BecomeCopy { .. }))
        .expect("Shuri must have a BecomeCopy activated ability");

    // CR 601.2c: targets are declared in printed order — recipient first, then
    // the copy source. The driver answers one slot per declared object, in
    // order, so passing two objects proves two slots were surfaced: if the
    // ability still declared only one slot, the second intent would go
    // unconsumed and the recipient would never receive the copy.
    let outcome = runner
        .activate(shuri, ability_index)
        .target_objects(&[recipient, donor])
        .resolve();

    let state = outcome.state();

    // CR 707.2: the RECIPIENT took on the donor's copiable name.
    assert_eq!(
        state.objects[&recipient].name, state.objects[&donor].name,
        "the targeted recipient must become a copy of the second target"
    );

    // The reported bug, pinned directly: Shuri must be untouched.
    assert_eq!(
        state.objects[&shuri].name, shuri_name_before,
        "Shuri is the ability's SOURCE, not its recipient — she must not become the copy"
    );

    // Reach guard (paired with the negative above): the ability actually
    // resolved rather than fizzling before the copy was installed, which would
    // make "Shuri is unchanged" pass for the wrong reason.
    assert!(
        matches!(outcome.final_waiting_for(), WaitingFor::Priority { .. }),
        "activation must resolve to a priority window, got {:?}",
        outcome.final_waiting_for()
    );
}
