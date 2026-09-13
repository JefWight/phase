//! CR 603.7c + CR 608.2k: a PHASE-delayed trigger that names an object carried
//! by its CREATION event must snapshot that object at creation time.
//!
//! "Whenever <source> deals combat damage to a creature, destroy that creature
//! AT END OF COMBAT" lowers to
//! `CreateDelayedTrigger { condition: AtNextPhase(EndCombat), effect: Destroy {
//! target: EventTarget } }`. `EventTarget` resolves out of
//! `state.current_trigger_event`, which at the end-of-combat step is the phase
//! change — it carries no object, so the referent resolved to nothing and the
//! destroy silently did nothing (issue #4229, Ohran Viper).
//!
//! `delayed_trigger::resolve` already creation-time-snapshots the OTHER
//! event-subject anaphor, `TriggeringSource`, for exactly this reason.
//! `EventTarget` is its CR 120.3 recipient counterpart and was simply never a
//! member of that snapshot pass.
//!
//! These are building-block tests: the snapshot is keyed on the anaphor, not on
//! a card, so the coverage below spans the self-referential source (Ohran
//! Viper), a source filter that is NOT the damage dealer (the Sliver class,
//! where the trigger source watches a third object deal the damage), and the
//! granted-ability form (Simic Basilisk), whose trigger source is the creature
//! that received the grant rather than the granter.

use super::rules::{GameScenario, Phase, P0, P1};
use engine::game::combat::AttackTarget;
use engine::game::scenario::GameRunner;
use engine::types::actions::GameAction;
use engine::types::identifiers::ObjectId;
use engine::types::mana::{ManaType, ManaUnit};
use engine::types::zones::Zone;

const OHRAN_VIPER: &str =
    "Whenever this creature deals combat damage to a creature, destroy that creature at end of combat.";
const DELAYED_SLIVER: &str =
    "Whenever a Sliver deals combat damage to a creature, destroy that creature at end of combat.";
const SIMIC_BASILISK_GRANT: &str = "{1}{G}: Until end of turn, target creature with a +1/+1 counter on it gains \"Whenever this creature deals combat damage to a creature, destroy that creature at end of combat.\"";
/// Verbatim Oracle text (Scryfall). An instant, so it can be cast in the
/// combat-damage step's priority window — which is the only place a blink can
/// land BETWEEN the delayed trigger's creation and its end-of-combat firing.
/// "Creature you control" is satisfiable here because the damage RECIPIENT is
/// by construction controlled by the trigger controller's opponent.
const EPHEMERATE: &str =
    "Exile target creature you control, then return it to the battlefield under its owner's control.";

/// CR 400.1: the zone an object currently occupies, read straight off the live
/// runner (`Outcome::zone_of` only sees the snapshot taken at its own call).
fn zone_of(runner: &GameRunner, object: ObjectId) -> Zone {
    runner.state().objects[&object].zone
}

/// One unit of untapped, unrestricted mana of `color`.
fn mana(color: ManaType) -> ManaUnit {
    ManaUnit::new(color, ObjectId(0), false, vec![])
}

/// Drive the declare-blockers step: pass priority until the engine surfaces it.
fn pass_into_declare_blockers(runner: &mut GameRunner) {
    for _ in 0..8 {
        if runner.waiting_for_kind() == "DeclareBlockers" {
            return;
        }
        runner
            .act(GameAction::PassPriority)
            .expect("pass priority into the declare-blockers step");
    }
    panic!("never reached the declare-blockers step");
}

/// CR 511.1 + CR 603.7c: the reported board state (issue #4229). Ohran Viper
/// attacks, a 0/6 Wall blocks. The Viper deals 1 combat damage to the Wall — not
/// lethal, so only the delayed destroy can remove it — and takes 0 back. At the
/// END OF COMBAT step the delayed trigger fires and must destroy the Wall it
/// damaged.
#[test]
fn ohran_viper_destroys_the_damaged_creature_at_end_of_combat() {
    let mut scenario = GameScenario::new();
    scenario.at_phase(Phase::PreCombatMain);

    let viper = {
        let mut b = scenario.add_creature(P0, "Ohran Viper", 1, 2);
        b.from_oracle_text(OHRAN_VIPER);
        b.id()
    };
    let wall = scenario.add_creature(P1, "Wall of Stone", 0, 6).id();

    let mut runner = scenario.build();
    runner.advance_to_combat();
    runner
        .declare_attackers(&[(viper, AttackTarget::Player(P1))])
        .expect("declare attackers");
    pass_into_declare_blockers(&mut runner);
    runner
        .declare_blockers(&[(wall, viper)])
        .expect("declare blockers");

    let damage = runner.combat_damage();
    assert_eq!(
        damage.zone_of(wall),
        Zone::Battlefield,
        "CR 704.5g: 1 damage is not lethal to a 0/6 — the Wall may only die to the \
         delayed destroy, so this test would pass vacuously if it died here"
    );

    // CR 511.1: the delayed trigger fires at the beginning of the end-of-combat
    // step; drive past it so the trigger resolves.
    runner.advance_to_phase(Phase::PostCombatMain);

    assert_eq!(
        zone_of(&runner, wall),
        Zone::Graveyard,
        "CR 603.7c: the delayed destroy must affect the creature the trigger's \
         CREATION event damaged, snapshotted at creation time"
    );
    assert_eq!(
        zone_of(&runner, viper),
        Zone::Battlefield,
        "CR 120.1: the damage DEALER is not its own referent"
    );
}

/// CR 120.3 + CR 603.7c: the snapshot must bind the event's damage RECIPIENT,
/// not the trigger source and not the ability's source object. Here the trigger
/// source (a 3/3 Sliver lord that never fights) watches a DIFFERENT Sliver deal
/// the combat damage, so a snapshot keyed on `ability.source_id` or on
/// `TriggeringSource` would destroy the wrong creature — or nothing at all.
#[test]
fn delayed_sliver_trigger_destroys_the_recipient_not_the_source() {
    let mut scenario = GameScenario::new();
    scenario.at_phase(Phase::PreCombatMain);

    let watcher = {
        let mut b = scenario.add_creature(P0, "Toxin Sliver", 3, 3);
        b.with_subtypes(vec!["Sliver"]);
        b.from_oracle_text(DELAYED_SLIVER);
        b.id()
    };
    let dealer = {
        let mut b = scenario.add_creature(P0, "Sidewinder Sliver", 1, 1);
        b.with_subtypes(vec!["Sliver"]);
        b.id()
    };
    let blocker = scenario.add_creature(P1, "Wall of Stone", 0, 6).id();

    let mut runner = scenario.build();
    runner.advance_to_combat();
    runner
        .declare_attackers(&[(dealer, AttackTarget::Player(P1))])
        .expect("declare attackers");
    pass_into_declare_blockers(&mut runner);
    runner
        .declare_blockers(&[(blocker, dealer)])
        .expect("declare blockers");

    let damage = runner.combat_damage();
    assert_eq!(
        damage.zone_of(blocker),
        Zone::Battlefield,
        "1 damage is not lethal to a 0/6 — guards against a vacuous pass"
    );

    runner.advance_to_phase(Phase::PostCombatMain);

    assert_eq!(
        zone_of(&runner, blocker),
        Zone::Graveyard,
        "CR 120.3: the delayed destroy names the damage RECIPIENT"
    );
    assert_eq!(
        zone_of(&runner, dealer),
        Zone::Battlefield,
        "CR 120.1: the damage dealer must survive — `TriggeringSource` is the wrong anaphor"
    );
    assert_eq!(
        zone_of(&runner, watcher),
        Zone::Battlefield,
        "the trigger source is not its own referent"
    );
}

/// CR 603.7c + CR 113.3 + CR 611.2c: the GRANTED form (Simic Basilisk). The
/// trigger is printed on the grantor's activated ability and granted to a
/// DIFFERENT creature, so the delayed trigger created at combat damage belongs
/// to the GRANTEE. The creation-time snapshot must read the grantee's own
/// creation event — anything keyed on the grantor (which never fought) resolves
/// to nothing.
#[test]
fn granted_delayed_damage_trigger_snapshots_its_own_creation_event() {
    let mut scenario = GameScenario::new();
    scenario.at_phase(Phase::PreCombatMain);
    scenario.with_mana_pool(P0, vec![mana(ManaType::Green), mana(ManaType::Green)]);

    // The grantor stays home: it never attacks and never deals damage.
    let grantor = {
        let mut b = scenario.add_creature(P0, "Simic Basilisk", 2, 2);
        b.from_oracle_text(SIMIC_BASILISK_GRANT);
        b.id()
    };
    // CR 122.1: the grant targets "creature with a +1/+1 counter on it".
    let grantee = {
        let mut b = scenario.add_creature(P0, "Grafted Creature", 1, 1);
        b.with_plus_counters(1);
        b.id()
    };
    let blocker = scenario.add_creature(P1, "Wall of Stone", 0, 6).id();

    let mut runner = scenario.build();
    runner.activate(grantor, 0).target_object(grantee).resolve();

    runner.advance_to_combat();
    runner
        .declare_attackers(&[(grantee, AttackTarget::Player(P1))])
        .expect("declare attackers");
    pass_into_declare_blockers(&mut runner);
    runner
        .declare_blockers(&[(blocker, grantee)])
        .expect("declare blockers");

    let damage = runner.combat_damage();
    assert_eq!(
        damage.zone_of(blocker),
        Zone::Battlefield,
        "2 damage is not lethal to a 0/6 — guards against a vacuous pass"
    );

    runner.advance_to_phase(Phase::PostCombatMain);

    assert_eq!(
        zone_of(&runner, blocker),
        Zone::Graveyard,
        "CR 603.7c: a granted delayed trigger snapshots the recipient from its own \
         creation event"
    );
    assert_eq!(
        zone_of(&runner, grantor),
        Zone::Battlefield,
        "the grantor never fought and is not its own referent"
    );
}

/// CR 400.7 + CR 603.7c: the PIN, exercised through the production pipeline.
///
/// "If that object leaves the battlefield and returns, it becomes a new object
/// and the ability no longer affects it." The creature is damaged (which creates
/// and snapshots the delayed trigger), then BLINKED in the combat-damage step's
/// priority window, and returns as a new incarnation before the end-of-combat
/// step fires the delayed destroy. The returned permanent must survive.
///
/// Two arms. Arm 1 is a mandatory reach-guard: without it, "the creature lived"
/// in arm 2 would also pass on a trigger that never fired, or on a snapshot pass
/// that silently bound nothing — the exact bug this file exists to catch.
///
/// The blink is cast by the RECIPIENT's controller, which is always the trigger
/// controller's opponent, so Ephemerate's "creature you control" is satisfied.
#[test]
fn delayed_destroy_does_not_affect_a_blinked_and_returned_recipient() {
    /// Drive combat until the damage trigger has RESOLVED and installed its
    /// delayed trigger, stopping in the combat-damage step's priority window
    /// rather than running on to end of combat.
    ///
    /// Stopping merely at "damage is marked" is too early: the damage trigger is
    /// still on the stack at that point and `CreateDelayedTrigger` has not run.
    fn deal_combat_damage_and_hold_priority(
        runner: &mut GameRunner,
        viper: ObjectId,
        wall: ObjectId,
    ) {
        runner.advance_to_combat();
        runner
            .declare_attackers(&[(viper, AttackTarget::Player(P1))])
            .expect("declare attackers");
        pass_into_declare_blockers(runner);
        runner
            .declare_blockers(&[(wall, viper)])
            .expect("declare blockers");
        for _ in 0..16 {
            if !runner.state().delayed_triggers.is_empty() {
                return;
            }
            assert_ne!(
                runner.state().phase,
                Phase::EndCombat,
                "reached end of combat before the damage trigger installed its \
                 delayed trigger — the blink would have nothing to race"
            );
            runner
                .act(GameAction::PassPriority)
                .expect("pass priority toward the combat damage step");
        }
        panic!(
            "combat damage never installed a delayed trigger (damage marked: {})",
            runner.state().objects[&wall].damage_marked
        );
    }

    // ---- Arm 1 (reach-guard): no blink, the delayed destroy DOES land. ----
    let mut scenario = GameScenario::new();
    scenario.at_phase(Phase::PreCombatMain);
    let viper = {
        let mut b = scenario.add_creature(P0, "Ohran Viper", 1, 2);
        b.from_oracle_text(OHRAN_VIPER);
        b.id()
    };
    let wall = scenario.add_creature(P1, "Wall of Stone", 0, 6).id();

    let mut runner = scenario.build();
    deal_combat_damage_and_hold_priority(&mut runner, viper, wall);
    assert_eq!(
        runner.state().delayed_triggers.len(),
        1,
        "reach-guard: combat damage must install exactly one delayed trigger"
    );
    runner.advance_to_phase(Phase::PostCombatMain);
    assert_eq!(
        zone_of(&runner, wall),
        Zone::Graveyard,
        "arm 1 reach-guard: with no blink the delayed destroy must kill the \
         damaged creature — otherwise arm 2 proves nothing"
    );

    // ---- Arm 2 (the detector): blink the recipient, it must SURVIVE. ----
    let mut scenario = GameScenario::new();
    scenario.at_phase(Phase::PreCombatMain);
    scenario.with_mana_pool(P1, vec![mana(ManaType::White), mana(ManaType::White)]);
    let viper = {
        let mut b = scenario.add_creature(P0, "Ohran Viper", 1, 2);
        b.from_oracle_text(OHRAN_VIPER);
        b.id()
    };
    let wall = scenario.add_creature(P1, "Wall of Stone", 0, 6).id();
    let ephemerate = scenario
        .add_spell_to_hand_from_oracle(P1, "Ephemerate", true, EPHEMERATE)
        .id();

    let mut runner = scenario.build();
    deal_combat_damage_and_hold_priority(&mut runner, viper, wall);
    assert_eq!(
        runner.state().delayed_triggers.len(),
        1,
        "reach-guard: the blink arm must install the same delayed trigger"
    );

    // The recipient's controller (P1) holds priority after the active player
    // passes; cast the blink there.
    for _ in 0..4 {
        if runner.state().waiting_for.acting_players().first().copied() == Some(P1) {
            break;
        }
        runner
            .act(GameAction::PassPriority)
            .expect("pass active-player priority so the blinker can respond");
    }
    let blinked = runner.cast(ephemerate).target_object(wall).resolve();
    assert_eq!(
        blinked.zone_of(wall),
        Zone::Battlefield,
        "reach-guard: Ephemerate must return the creature to the battlefield"
    );

    runner.advance_to_phase(Phase::PostCombatMain);

    assert_eq!(
        zone_of(&runner, wall),
        Zone::Battlefield,
        "CR 400.7: the blinked recipient came back as a NEW object, so the \
         pinned delayed destroy must not affect it"
    );
}
