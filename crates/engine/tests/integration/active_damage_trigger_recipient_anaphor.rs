//! CR 120.1 + CR 120.3 + CR 608.2k: the ACTIVE-voice damage trigger with an
//! OBJECT recipient — "Whenever <source> deals [combat] damage to a creature,
//! <do something to> that creature".
//!
//! Reported from live play with a Sliver deck: a Striking Sliver (1/1, granting
//! first strike) blocked a 2/2 attacker while a Toxin Sliver was on the
//! battlefield. Toxin Sliver's trigger fired and put a Destroy on the stack, but
//! the attacker survived the first-strike step and killed the blocker in the
//! regular combat damage step.
//!
//! Cause: "that creature" bound to `TargetFilter::ParentTarget`. An untargeted
//! damage trigger has no chosen target, and the parentless-`ParentTarget` event
//! fallback in `game/targeting.rs` carries no `DamageDealt` arm, so the referent
//! resolved to NO object and the effect silently did nothing. The
//! subject-derived `TriggeringSource` fallback would have been just as wrong in
//! the other direction: on an ACTIVE-voice condition the event's `source_id` is
//! the damage DEALER (CR 120.1), so "destroy that creature" would have destroyed
//! the blocking Sliver instead of the attacker.
//!
//! The referent is the damage RECIPIENT — `TargetFilter::EventTarget`.
//!
//! These are building-block tests: the parser assertions cover the whole printed
//! class across four different effect verbs (destroy / exile / tap / put a
//! counter) and three source-filter shapes (self-referential by name, a subtype
//! filter, a controller-scoped filter), not one card's text.

use super::rules::{GameScenario, Phase, P0, P1};
use engine::game::combat::AttackTarget;
use engine::types::ability::{Effect, TargetFilter};
use engine::types::actions::GameAction;
use engine::types::zones::Zone;

const TOXIN_SLIVER: &str = "Whenever a Sliver deals combat damage to a creature, destroy that creature. It can't be regenerated.";
const STRIKING_SLIVER: &str = "Sliver creatures you control have first strike.";

/// Parse a single trigger line and return the top-level effect of its body.
fn trigger_body_effect(oracle: &str, card_name: &str) -> Effect {
    let abilities = engine::parser::oracle::parse_oracle_text(oracle, card_name, &[], &[], &[]);
    let trigger = abilities
        .triggers
        .first()
        .unwrap_or_else(|| panic!("{card_name}: expected a triggered ability"));
    let execute = trigger
        .execute
        .as_deref()
        .unwrap_or_else(|| panic!("{card_name}: trigger has no body"));
    (*execute.effect).clone()
}

/// The primary object referent of an effect, for the four verbs this class uses.
fn effect_subject(effect: &Effect) -> &TargetFilter {
    match effect {
        Effect::Destroy { target, .. }
        | Effect::ChangeZone { target, .. }
        | Effect::SetTapState { target, .. }
        | Effect::PutCounter { target, .. } => target,
        other => panic!("unexpected effect shape: {other:?}"),
    }
}

/// CR 608.2k: "that creature" in an active-voice damage trigger names the damage
/// RECIPIENT, across every effect verb and every source-filter shape in the
/// class. Binding it to `ParentTarget` (no referent on an untargeted trigger) or
/// to `TriggeringSource` (the damage dealer) are both wrong.
#[test]
fn active_damage_trigger_demonstrative_binds_the_damage_recipient() {
    for (card, oracle) in [
        // Subtype-filtered source.
        ("Toxin Sliver", TOXIN_SLIVER),
        // Self-referential source by name — the recipient is still a DIFFERENT
        // object, so the self-referential subject must not suppress the binding
        // the way it does for the passive-voice ("is dealt damage") class.
        (
            "Phage the Untouchable",
            "Whenever Phage deals combat damage to a creature, destroy that creature. It can't be regenerated.",
        ),
        (
            "Stinkweed Imp",
            "Whenever this creature deals combat damage to a creature, destroy that creature.",
        ),
        // Noncombat-inclusive ("deals damage", no kind adjective) + exile verb.
        (
            "Pit Spawn",
            "Whenever this creature deals damage to a creature, exile that creature.",
        ),
        // Attachment source subject.
        (
            "Sword of Kaldra",
            "Whenever equipped creature deals damage to a creature, exile that creature.",
        ),
        // Tap verb.
        (
            "Kashi-Tribe Elite",
            "Whenever this creature deals combat damage to a creature, tap that creature and it doesn't untap during its controller's next untap step.",
        ),
        // Counter verb.
        (
            "Obelisk Spider",
            "Whenever this creature deals combat damage to a creature, put a -1/-1 counter on that creature.",
        ),
        // Controller-scoped source filter.
        (
            "Quest for the Gemblades class",
            "Whenever a creature you control deals combat damage to a creature, exile that creature.",
        ),
    ] {
        let effect = trigger_body_effect(oracle, card);
        assert_eq!(
            effect_subject(&effect),
            &TargetFilter::EventTarget,
            "{card}: \"that creature\" must bind the damage recipient"
        );
    }
}

/// CR 120.1: the PLAYER-recipient sibling must be untouched — "deals combat
/// damage to a player" names no object recipient, so the object anaphor pin must
/// not fire and "that player" keeps its own player-scope binding.
#[test]
fn player_recipient_damage_trigger_keeps_its_player_anaphor() {
    let abilities = engine::parser::oracle::parse_oracle_text(
        "Whenever this creature deals combat damage to a player, that player discards a card.",
        "Player Recipient",
        &[],
        &[],
        &[],
    );
    let trigger = abilities.triggers.first().expect("triggered ability");
    let execute = trigger.execute.as_deref().expect("trigger body");
    let Effect::Discard { target, .. } = &*execute.effect else {
        panic!("expected a Discard body, got {:?}", execute.effect);
    };
    assert_eq!(
        target,
        &TargetFilter::TriggeringPlayer,
        "a player recipient must not be captured by the object anaphor pin"
    );
}

/// CR 510.4 + CR 702.7b + CR 701.8a: the reported board state end to end. A 1/1
/// Striking Sliver blocks a 2/2 attacker while a Toxin Sliver watches. First
/// strike means the Sliver's 1 damage lands in the first-strike combat damage
/// step; Toxin Sliver's trigger resolves before the regular combat damage step,
/// so the attacker is destroyed and never deals its damage back.
#[test]
fn toxin_sliver_destroys_a_creature_damaged_by_another_sliver() {
    let mut scenario = GameScenario::new();
    scenario.at_phase(Phase::PreCombatMain);

    let toxin = {
        let mut b = scenario.add_creature(P0, "Toxin Sliver", 3, 3);
        b.with_subtypes(vec!["Sliver"]);
        b.from_oracle_text(TOXIN_SLIVER);
        b.id()
    };
    let striking = {
        let mut b = scenario.add_creature(P0, "Striking Sliver", 1, 1);
        b.with_subtypes(vec!["Sliver"]);
        b.from_oracle_text(STRIKING_SLIVER);
        b.id()
    };
    let attacker = scenario.add_creature(P1, "Grizzly Bears", 2, 2).id();

    let mut runner = scenario.build();
    runner.state_mut().active_player = P1;
    runner.advance_to_combat();
    runner
        .declare_attackers(&[(attacker, AttackTarget::Player(P0))])
        .expect("declare attackers");
    for _ in 0..8 {
        if runner.waiting_for_kind() == "DeclareBlockers" {
            break;
        }
        runner
            .act(GameAction::PassPriority)
            .expect("pass priority into the declare-blockers step");
    }
    runner
        .declare_blockers(&[(striking, attacker)])
        .expect("declare blockers");

    let outcome = runner.combat_damage();

    assert_eq!(
        outcome.zone_of(attacker),
        Zone::Graveyard,
        "CR 701.8a: the creature dealt combat damage by a Sliver must be destroyed"
    );
    assert_eq!(
        outcome.zone_of(striking),
        Zone::Battlefield,
        "CR 510.4: the attacker is destroyed in the first-strike step, so it never \
         deals combat damage back to the 1/1 blocker"
    );
    assert_eq!(
        outcome.zone_of(toxin),
        Zone::Battlefield,
        "the trigger source is not its own referent"
    );
}
