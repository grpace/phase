//! Star Trek (TRK) cards unlocked by three shared parser fixes, proved through
//! the live cast → trigger → resolve pipeline with verbatim MTGJSON Oracle text:
//!
//! - Commander Raffi Musiker — printed short self-name "Commander Musiker"
//!   (CR 201.5c) on a modal ETB trigger.
//! - Worf, Intelligence Contractor — plural registered subtype "Warriors"
//!   becoming the `Warrior` creature type (CR 205.3m / CR 205.1b).
//! - Those Old Scientists — Saga whose reminder text spans a line break
//!   (MTGJSON prints "after your draw\nstep"), so chapter headers must still
//!   be recognized (CR 714.2b); chapter III shuffles each distinct owner's
//!   library (CR 701.24a).
//! - Ensign Harry Kim, Operations — printed short self-name "Ensign Kim".
//! - New New York — "become 3/3 Vehicles" adds no creature type (CR 301.7a).
//! - Dr. Leonard "Bones" McCoy — an embedded "When it dies this turn, …"
//!   delayed trigger keeps its trailing "at the beginning of the next end
//!   step" on its own body (CR 603.7), and its bare "it" names the creature
//!   the mode just chose (CR 608.2c). The watch is bounded to the turn
//!   (CR 603.7b) and is not installed when no creature was chosen.
//!   Desperate Measures and Turn Inside Out cover the same pronoun binding and
//!   duration on instants.
//!
//! Every test builds its card from the verbatim Oracle text through
//! `from_oracle_text`, so it exercises the same parser the card-data export
//! uses; reverting any of the three fixes changes the parse these tests drive.

use engine::game::scenario::{GameRunner, GameScenario, P0, P1};
use engine::game::triggers::drain_order_triggers_with_identity;
use engine::types::ability::{EffectKind, TargetRef};
use engine::types::actions::GameAction;
use engine::types::card_type::CoreType;
use engine::types::counter::CounterType;
use engine::types::events::{GameEvent, PlayerActionKind};
use engine::types::game_state::{
    CastPaymentMode, CounterCostChoice, PayCostKind, TargetSelectionSlot, WaitingFor,
};
use engine::types::identifiers::ObjectId;
use engine::types::keywords::Keyword;
use engine::types::mana::{ManaCost, ManaType, ManaUnit};
use engine::types::phase::Phase;
use engine::types::player::PlayerId;
use engine::types::zones::Zone;

const MUSIKER_NAME: &str = "Commander Raffi Musiker";
const MUSIKER_ORACLE: &str = "When Commander Musiker enters, choose one —\n• Exile up to one target card from a graveyard. You gain 2 life.\n• Investigate. You lose 2 life. (Create a Clue token. It's an artifact with \"{2}, Sacrifice this token: Draw a card.\")";

const WORF_NAME: &str = "Worf, Intelligence Contractor";
const WORF_ORACLE: &str = "When Worf enters, for each opponent, gain control of up to one target creature that player controls with mana value 3 or less until end of turn. Untap those creatures. Until end of turn, they become Warriors in addition to their other types and gain haste.";

const SCIENTISTS_NAME: &str = "Those Old Scientists";
/// Verbatim MTGJSON text, including the line break inside the reminder text.
const SCIENTISTS_ORACLE: &str = "(As this Saga enters and after your draw\nstep, add a lore counter. Sacrifice after III.)\nI — Draw two cards, then discard two cards.\nII — Return up to one target artifact, instant, or sorcery card from your graveyard to your hand.\nIII — Choose up to two target cards in graveyards. The owners of those cards shuffle them into their libraries.";

// ---------------------------------------------------------------------------
// Shared drivers
// ---------------------------------------------------------------------------

fn cast_from_hand(runner: &mut GameRunner, object_id: ObjectId) {
    let card_id = runner.state().objects[&object_id].card_id;
    runner
        .act(GameAction::CastSpell {
            object_id,
            card_id,
            targets: vec![],
            payment_mode: CastPaymentMode::Auto,
        })
        .expect("cast from hand");
}

/// Pass priority (draining trigger ordering) until the game stops at a
/// non-priority prompt, or reaches priority with an empty stack.
fn pass_until_prompt_or_empty(runner: &mut GameRunner) {
    for _ in 0..64 {
        match runner.state().waiting_for.clone() {
            WaitingFor::Priority { .. } if runner.state().stack.is_empty() => return,
            WaitingFor::Priority { .. } => {
                runner.act(GameAction::PassPriority).expect("pass priority");
            }
            WaitingFor::OrderTriggers { .. } => {
                drain_order_triggers_with_identity(runner.state_mut());
            }
            _ => return,
        }
    }
    panic!("pipeline did not settle");
}

/// Assert the game fully settled: empty stack and priority with the active player.
fn assert_settled(runner: &GameRunner) {
    assert!(runner.state().stack.is_empty(), "stack must be empty");
    assert!(
        matches!(runner.state().waiting_for, WaitingFor::Priority { player } if player == runner.state().active_player),
        "expected active-player priority, got {:?}",
        runner.state().waiting_for
    );
}

/// Unconditionally take the current trigger target prompt's slots.
fn trigger_target_slots(runner: &GameRunner) -> (PlayerId, Vec<TargetSelectionSlot>) {
    match &runner.state().waiting_for {
        WaitingFor::TriggerTargetSelection {
            player,
            target_slots,
            ..
        } => (*player, target_slots.clone()),
        other => panic!("expected TriggerTargetSelection, got {other:?}"),
    }
}

fn current_slot_legal(runner: &GameRunner) -> Vec<TargetRef> {
    match &runner.state().waiting_for {
        WaitingFor::TriggerTargetSelection {
            target_slots,
            selection,
            ..
        } => target_slots[selection.current_slot].legal_targets.clone(),
        other => panic!("expected TriggerTargetSelection, got {other:?}"),
    }
}

fn choose(runner: &mut GameRunner, target: Option<ObjectId>) {
    runner
        .act(GameAction::ChooseTarget {
            target: target.map(TargetRef::Object),
        })
        .expect("target choice accepted");
}

fn lore(runner: &GameRunner, id: ObjectId) -> u32 {
    runner.state().objects[&id]
        .counters
        .get(&CounterType::Lore)
        .copied()
        .unwrap_or(0)
}

/// Drive the turn structure through public actions (passing priority and
/// declaring no attackers or blockers) until `done` holds, stopping early at
/// any other prompt.
fn advance_until(runner: &mut GameRunner, done: impl Fn(&GameRunner) -> bool) {
    for _ in 0..256 {
        if done(runner) {
            return;
        }
        match runner.state().waiting_for.clone() {
            WaitingFor::Priority { .. } => {
                runner.act(GameAction::PassPriority).expect("pass priority");
            }
            WaitingFor::DeclareAttackers { .. } => {
                runner
                    .act(GameAction::DeclareAttackers {
                        attacks: vec![],
                        bands: vec![],
                    })
                    .expect("declare no attackers");
            }
            WaitingFor::DeclareBlockers { .. } => {
                runner
                    .act(GameAction::DeclareBlockers {
                        assignments: vec![],
                    })
                    .expect("declare no blockers");
            }
            WaitingFor::OrderTriggers { .. } => {
                drain_order_triggers_with_identity(runner.state_mut());
            }
            other => panic!(
                "unexpected prompt while advancing (turn {}, {:?}): {other:?}",
                runner.state().turn_number,
                runner.state().phase
            ),
        }
    }
    panic!("turn structure did not reach the requested point");
}

/// Advance to `player`'s next turn, stopping as soon as that turn's precombat
/// main phase begins (before any chapter trigger there is answered).
fn advance_to_next_main_of(runner: &mut GameRunner, player: PlayerId) {
    let start_turn = runner.state().turn_number;
    advance_until(runner, |r| {
        r.state().turn_number > start_turn
            && r.state().active_player == player
            && r.state().phase == Phase::PreCombatMain
    });
}

// ---------------------------------------------------------------------------
// Commander Raffi Musiker
// ---------------------------------------------------------------------------

fn add_musiker_to_hand(scenario: &mut GameScenario) -> ObjectId {
    scenario
        .add_creature_to_hand_from_oracle(P0, MUSIKER_NAME, 3, 2, MUSIKER_ORACLE)
        .with_mana_cost(ManaCost::zero())
        .id()
}

/// Cast Musiker and drive to the ETB trigger's mode choice; assert the prompt.
fn cast_musiker_to_mode_choice(runner: &mut GameRunner, musiker: ObjectId) {
    cast_from_hand(runner, musiker);
    pass_until_prompt_or_empty(runner);
    // CR 201.5c + CR 603.6a: "When Commander Musiker enters" refers to this
    // card, so its own entry triggers it. CR 700.2b: the controller chooses
    // the mode as the trigger is put on the stack.
    match &runner.state().waiting_for {
        WaitingFor::AbilityModeChoice {
            player,
            modal,
            source_id,
            ..
        } => {
            assert_eq!(*player, P0);
            assert_eq!(*source_id, musiker);
            assert_eq!(
                (modal.min_choices, modal.max_choices, modal.mode_count),
                (1, 1, 2),
                "choose one of two modes"
            );
        }
        other => panic!("expected Musiker's ETB mode choice, got {other:?}"),
    }
    assert_eq!(runner.state().objects[&musiker].zone, Zone::Battlefield);
}

#[test]
fn musiker_mode_one_exiles_chosen_graveyard_card_and_gains_two() {
    let mut scenario = GameScenario::new();
    scenario.at_phase(Phase::PreCombatMain);
    let musiker = add_musiker_to_hand(&mut scenario);
    let their_card = scenario
        .add_creature_to_graveyard(P1, "Fallen Ensign", 2, 2)
        .id();
    let my_card = scenario
        .add_spell_to_graveyard(P0, "Spent Order", true)
        .id();
    let mut runner = scenario.build();
    let life_before = runner.life(P0);

    cast_musiker_to_mode_choice(&mut runner, musiker);
    runner
        .act(GameAction::SelectModes { indices: vec![0] })
        .expect("select exile mode");

    // CR 115.1d: "up to one target card from a graveyard" — any graveyard.
    let (chooser, slots) = trigger_target_slots(&runner);
    assert_eq!(chooser, P0);
    assert_eq!(slots.len(), 1);
    assert!(slots[0].optional, "up to one target is optional");
    let legal = &slots[0].legal_targets;
    assert!(legal.contains(&TargetRef::Object(their_card)));
    assert!(legal.contains(&TargetRef::Object(my_card)));
    choose(&mut runner, Some(their_card));
    pass_until_prompt_or_empty(&mut runner);

    assert_settled(&runner);
    assert_eq!(runner.state().objects[&their_card].zone, Zone::Exile);
    assert_eq!(
        runner.state().objects[&my_card].zone,
        Zone::Graveyard,
        "only the chosen card is exiled"
    );
    assert_eq!(runner.life(P0), life_before + 2);
    assert_eq!(runner.life(P1), 20);
}

#[test]
fn musiker_mode_two_investigates_and_loses_two() {
    let mut scenario = GameScenario::new();
    scenario.at_phase(Phase::PreCombatMain);
    let musiker = add_musiker_to_hand(&mut scenario);
    let graveyard_card = scenario
        .add_creature_to_graveyard(P1, "Fallen Ensign", 2, 2)
        .id();
    let mut runner = scenario.build();
    let life_before = runner.life(P0);

    cast_musiker_to_mode_choice(&mut runner, musiker);
    runner
        .act(GameAction::SelectModes { indices: vec![1] })
        .expect("select investigate mode");
    pass_until_prompt_or_empty(&mut runner);

    assert_settled(&runner);
    // CR 701.16a + CR 111.10f: Investigate creates a Clue artifact token.
    let clues: Vec<_> = runner
        .state()
        .battlefield
        .iter()
        .filter_map(|id| runner.state().objects.get(id))
        .filter(|obj| obj.is_token && obj.card_types.subtypes.iter().any(|s| s == "Clue"))
        .collect();
    assert_eq!(clues.len(), 1, "exactly one Clue token");
    let clue = clues[0];
    assert_eq!(clue.controller, P0);
    assert!(clue.card_types.core_types.contains(&CoreType::Artifact));
    assert!(
        !clue.abilities.is_empty(),
        "Clue carries its sacrifice-to-draw ability"
    );
    assert_eq!(runner.life(P0), life_before - 2);
    assert_eq!(
        runner.state().objects[&graveyard_card].zone,
        Zone::Graveyard,
        "the exile mode did not run"
    );
}

#[test]
fn musiker_does_not_trigger_on_another_creature_entering() {
    let mut scenario = GameScenario::new();
    scenario.at_phase(Phase::PreCombatMain);
    let musiker = scenario
        .add_creature_from_oracle(P0, MUSIKER_NAME, 3, 2, MUSIKER_ORACLE)
        .id();
    let other = scenario
        .add_creature_to_hand(P0, "Away Team", 2, 2)
        .with_mana_cost(ManaCost::zero())
        .id();
    let mut runner = scenario.build();
    // Reach-guard: the battlefield Musiker carries the parsed ETB trigger that
    // the sibling tests prove fires on its own entry.
    assert_eq!(
        runner.state().objects[&musiker].trigger_definitions.len(),
        1,
        "Musiker must carry its ETB trigger"
    );
    let life_before = runner.life(P0);

    cast_from_hand(&mut runner, other);
    pass_until_prompt_or_empty(&mut runner);

    // CR 603.6a: "When [this object] enters" triggers only on that object.
    assert_eq!(runner.state().objects[&other].zone, Zone::Battlefield);
    assert_settled(&runner);
    assert_eq!(runner.life(P0), life_before);
    assert!(
        !runner.state().objects.values().any(|obj| obj.is_token),
        "no Clue was created"
    );
}

// ---------------------------------------------------------------------------
// Worf, Intelligence Contractor
// ---------------------------------------------------------------------------

/// Answer Worf's trigger target prompt, taking the next `picks` creature that
/// is legal at each prompt. CR 115.10a + CR 115.1d: the per-opponent "that
/// player" binder is not a target, so the engine announces it itself — every
/// prompt the controller sees must be an optional object-only creature slot.
/// Returns each prompted slot's legal targets, in prompt order.
fn drive_worf_targets(runner: &mut GameRunner, picks: &[ObjectId]) -> Vec<Vec<TargetRef>> {
    let (chooser, _) = trigger_target_slots(runner);
    assert_eq!(chooser, P0);
    let mut remaining: Vec<ObjectId> = picks.to_vec();
    let mut prompts = Vec::new();
    for _ in 0..16 {
        let WaitingFor::TriggerTargetSelection {
            target_slots,
            selection,
            ..
        } = &runner.state().waiting_for
        else {
            break;
        };
        let slot = &target_slots[selection.current_slot];
        assert!(slot.optional, "\"up to one target creature\" is optional");
        assert!(
            !slot.legal_targets.is_empty()
                && slot
                    .legal_targets
                    .iter()
                    .all(|t| matches!(t, TargetRef::Object(_))),
            "every prompt is an object-only creature slot: {:?}",
            slot.legal_targets
        );
        let legal = slot.legal_targets.clone();
        let pick = remaining
            .iter()
            .position(|id| legal.contains(&TargetRef::Object(*id)))
            .map(|i| remaining.remove(i));
        prompts.push(legal);
        choose(runner, pick);
    }
    assert!(
        remaining.is_empty(),
        "every intended creature must be targetable; unpicked: {remaining:?}"
    );
    prompts
}

/// An object-only slot's legal targets as a sorted id set.
fn object_set(targets: &[TargetRef]) -> Vec<ObjectId> {
    let mut ids: Vec<ObjectId> = targets
        .iter()
        .map(|t| match t {
            TargetRef::Object(id) => *id,
            TargetRef::Player(p) => panic!("unexpected player target {p:?}"),
        })
        .collect();
    ids.sort_by_key(|id| id.0);
    ids
}

#[test]
fn worf_steals_untaps_and_makes_a_hasty_warrior_until_end_of_turn() {
    let mut scenario = GameScenario::new();
    scenario.at_phase(Phase::PreCombatMain);
    for i in 0..4 {
        scenario.add_spell_to_library_top(P0, &format!("P0 Library {i}"), true);
        scenario.add_spell_to_library_top(P1, &format!("P1 Library {i}"), true);
    }
    let worf = scenario
        .add_creature_to_hand_from_oracle(P0, WORF_NAME, 3, 2, WORF_ORACLE)
        .with_mana_cost(ManaCost::zero())
        .id();
    let small = scenario
        .add_creature(P1, "Ensign Bear", 2, 2)
        .with_subtypes(vec!["Bear"])
        .with_mana_cost(ManaCost::generic(3))
        .id();
    let big = scenario
        .add_creature(P1, "Big Bear", 4, 4)
        .with_mana_cost(ManaCost::generic(4))
        .id();
    let mine = scenario
        .add_creature(P0, "Own Bear", 2, 2)
        .with_mana_cost(ManaCost::generic(1))
        .id();
    let mut runner = scenario.build();
    runner.state_mut().objects.get_mut(&small).unwrap().tapped = true;

    cast_from_hand(&mut runner, worf);
    pass_until_prompt_or_empty(&mut runner);

    // CR 115.1d: "up to one target creature that player controls with mana
    // value 3 or less" — only the opponent's MV ≤ 3 creature is legal.
    let creature_slots = drive_worf_targets(&mut runner, &[small]);
    // Exactly one creature slot for the one opponent, offering exactly that
    // opponent's MV ≤ 3 creature — not the MV 4 one, nor Worf's controller's.
    assert_eq!(
        creature_slots,
        vec![vec![TargetRef::Object(small)]],
        "big = {big:?}, mine = {mine:?}"
    );
    pass_until_prompt_or_empty(&mut runner);
    assert_settled(&runner);

    {
        let stolen = &runner.state().objects[&small];
        assert_eq!(stolen.controller, P0, "Worf's controller gains control");
        assert!(!stolen.tapped, "the stolen creature is untapped");
        // CR 205.1b + CR 613.1d: becomes a Warrior in addition to its types.
        assert!(stolen.card_types.subtypes.iter().any(|s| s == "Warrior"));
        assert!(stolen.card_types.subtypes.iter().any(|s| s == "Bear"));
        assert!(stolen.card_types.core_types.contains(&CoreType::Creature));
        assert!(stolen.has_keyword(&Keyword::Haste));
    }
    let untouched = &runner.state().objects[&big];
    assert_eq!(untouched.controller, P1);
    assert!(!untouched.card_types.subtypes.iter().any(|s| s == "Warrior"));
    assert!(!untouched.has_keyword(&Keyword::Haste));

    // CR 514.2 + CR 611.2a: the "until end of turn" effects end at cleanup.
    let start_turn = runner.state().turn_number;
    advance_until(&mut runner, |r| {
        r.state().turn_number > start_turn && r.state().phase == Phase::Upkeep
    });
    assert_eq!(runner.state().active_player, P1);
    let returned = &runner.state().objects[&small];
    assert_eq!(returned.controller, P1, "control returns at end of turn");
    assert!(!returned.card_types.subtypes.iter().any(|s| s == "Warrior"));
    assert!(returned.card_types.subtypes.iter().any(|s| s == "Bear"));
    assert!(!returned.has_keyword(&Keyword::Haste));
}

/// CR 115.1d: "for each opponent, ... up to one target creature
/// that player controls" — with two opponents Worf gets exactly one creature
/// slot per opponent, each offering exactly that opponent's MV ≤ 3 creatures,
/// and can take one creature from each of them.
#[test]
fn worf_takes_one_creature_from_each_opponent() {
    let mut scenario = GameScenario::new_n_player(3, 42);
    scenario.at_phase(Phase::PreCombatMain);
    let p2 = PlayerId(2);
    let worf = scenario
        .add_creature_to_hand_from_oracle(P0, WORF_NAME, 3, 2, WORF_ORACLE)
        .with_mana_cost(ManaCost::zero())
        .id();
    let mut creature = |owner: PlayerId, name: &str, mv: u32| {
        scenario
            .add_creature(owner, name, 2, 2)
            .with_mana_cost(ManaCost::generic(mv))
            .id()
    };
    let from_p1 = creature(P1, "Left Bear", 2);
    let other_p1 = creature(P1, "Left Cub", 3);
    let big_p1 = creature(P1, "Left Giant", 4);
    let from_p2 = creature(p2, "Right Bear", 1);
    let big_p2 = creature(p2, "Right Giant", 5);
    let mine = creature(P0, "Own Bear", 1);
    let mut runner = scenario.build();

    cast_from_hand(&mut runner, worf);
    pass_until_prompt_or_empty(&mut runner);
    let prompts = drive_worf_targets(&mut runner, &[from_p1, from_p2]);
    let mut prompts: Vec<Vec<ObjectId>> = prompts.iter().map(|p| object_set(p)).collect();
    prompts.sort_by_key(|set| set.first().map(|id| id.0));
    let mut expected = vec![
        object_set(&[TargetRef::Object(from_p1), TargetRef::Object(other_p1)]),
        vec![from_p2],
    ];
    expected.sort_by_key(|set| set.first().map(|id| id.0));
    assert_eq!(
        prompts, expected,
        "one slot per opponent, each exactly that opponent's MV ≤ 3 creatures \
         (excluded: {big_p1:?}, {big_p2:?}, {mine:?})"
    );
    pass_until_prompt_or_empty(&mut runner);
    assert_settled(&runner);

    for id in [from_p1, from_p2] {
        let obj = &runner.state().objects[&id];
        assert_eq!(obj.controller, P0, "{} was stolen", obj.name);
        assert!(obj.card_types.subtypes.iter().any(|s| s == "Warrior"));
        assert!(obj.has_keyword(&Keyword::Haste));
    }
    for id in [other_p1, big_p1, big_p2] {
        let obj = &runner.state().objects[&id];
        assert_ne!(obj.controller, P0, "{} was not chosen", obj.name);
        assert!(!obj.card_types.subtypes.iter().any(|s| s == "Warrior"));
    }
}

// ---------------------------------------------------------------------------
// Those Old Scientists
// ---------------------------------------------------------------------------

fn add_scientists_to_hand(scenario: &mut GameScenario) -> ObjectId {
    let mut builder = scenario.add_spell_to_hand(P0, SCIENTISTS_NAME, false);
    builder
        .as_enchantment()
        .with_subtypes(vec!["Saga"])
        .with_mana_cost(ManaCost::zero())
        .from_oracle_text(SCIENTISTS_ORACLE);
    builder.id()
}

/// Cast the Saga and answer chapter I's discard prompt by discarding every
/// offered card except the one named `kept_name`; returns the discarded ids.
fn cast_scientists_through_chapter_one(
    runner: &mut GameRunner,
    saga: ObjectId,
    kept_name: &str,
) -> Vec<ObjectId> {
    cast_from_hand(runner, saga);
    pass_until_prompt_or_empty(runner);
    // CR 714.3a + CR 714.2b: enters with a lore counter, which triggers
    // chapter I; draw two, then the discard prompt.
    assert_eq!(runner.state().objects[&saga].zone, Zone::Battlefield);
    assert_eq!(lore(runner, saga), 1);
    let (player, count, cards) = match &runner.state().waiting_for {
        WaitingFor::DiscardChoice {
            player,
            count,
            cards,
            ..
        } => (*player, *count, cards.clone()),
        other => panic!("expected chapter I discard prompt, got {other:?}"),
    };
    assert_eq!(player, P0);
    assert_eq!(count, 2);
    assert_eq!(cards.len(), 3, "starting card + two drawn cards");
    let discard: Vec<ObjectId> = cards
        .iter()
        .copied()
        .filter(|id| runner.state().objects[id].name != kept_name)
        .collect();
    assert_eq!(discard.len(), 2);
    runner
        .act(GameAction::SelectCards {
            cards: discard.clone(),
        })
        .expect("discard two");
    pass_until_prompt_or_empty(runner);
    discard
}

#[test]
fn scientists_chapter_one_draws_two_then_discards_two_on_entry() {
    let mut scenario = GameScenario::new();
    scenario.at_phase(Phase::PreCombatMain);
    let saga = add_scientists_to_hand(&mut scenario);
    let keeper = scenario.add_spell_to_hand(P0, "Keeper", true).id();
    let not_drawn = scenario
        .add_spell_to_library_top(P0, "Not Drawn", true)
        .id();
    let second = scenario.add_spell_to_library_top(P0, "Drawn B", true).id();
    let first = scenario.add_spell_to_library_top(P0, "Drawn A", true).id();
    let mut runner = scenario.build();
    let discarded = cast_scientists_through_chapter_one(&mut runner, saga, "Keeper");

    assert_settled(&runner);
    // The top two library cards were drawn; the third stays in the library.
    for id in [first, second] {
        assert_ne!(runner.state().objects[&id].zone, Zone::Library);
    }
    assert_eq!(runner.state().objects[&not_drawn].zone, Zone::Library);
    for id in &discarded {
        assert_eq!(runner.state().objects[id].zone, Zone::Graveyard);
    }
    assert_eq!(runner.state().objects[&keeper].zone, Zone::Hand);
    assert_eq!(runner.state().players[0].hand.len(), 1);
    assert_eq!(lore(&runner, saga), 1);
    assert_eq!(runner.state().objects[&saga].zone, Zone::Battlefield);
}

#[test]
fn scientists_chapters_two_and_three_then_sacrifice() {
    let mut scenario = GameScenario::new();
    scenario.at_phase(Phase::PreCombatMain);
    let saga = add_scientists_to_hand(&mut scenario);
    scenario.add_spell_to_hand(P0, "Keeper", true);
    for i in 0..6 {
        scenario.add_spell_to_library_top(P0, &format!("P0 Library {i}"), true);
        scenario.add_spell_to_library_top(P1, &format!("P1 Library {i}"), true);
    }
    let my_creature_card = scenario
        .add_creature_to_graveyard(P0, "Retired Officer", 2, 2)
        .id();
    let their_card = scenario
        .add_creature_to_graveyard(P1, "Fallen Rival", 2, 2)
        .id();
    let mut runner = scenario.build();

    let discarded = cast_scientists_through_chapter_one(&mut runner, saga, "Keeper");
    assert_settled(&runner);
    let returned_instant = discarded[0];
    let reshuffled_mine = discarded[1];

    // CR 714.3c: next precombat main phase adds a lore counter → chapter II.
    advance_to_next_main_of(&mut runner, P0);
    assert_eq!(lore(&runner, saga), 2);
    pass_until_prompt_or_empty(&mut runner);
    let (chooser, slots) = trigger_target_slots(&runner);
    assert_eq!(chooser, P0);
    assert_eq!(slots.len(), 1);
    assert!(slots[0].optional, "up to one target is optional");
    let legal = &slots[0].legal_targets;
    assert!(legal.contains(&TargetRef::Object(returned_instant)));
    assert!(
        !legal.contains(&TargetRef::Object(my_creature_card)),
        "a creature card is not an artifact, instant, or sorcery card"
    );
    assert!(
        !legal.contains(&TargetRef::Object(their_card)),
        "only your own graveyard"
    );
    choose(&mut runner, Some(returned_instant));
    pass_until_prompt_or_empty(&mut runner);
    assert_settled(&runner);
    assert_eq!(runner.state().objects[&returned_instant].zone, Zone::Hand);
    assert_eq!(runner.state().objects[&saga].zone, Zone::Battlefield);

    // Chapter III on the following turn.
    advance_to_next_main_of(&mut runner, P0);
    assert_eq!(lore(&runner, saga), 3);
    pass_until_prompt_or_empty(&mut runner);
    let (chooser, _) = trigger_target_slots(&runner);
    assert_eq!(chooser, P0);
    let mut wanted = vec![their_card, reshuffled_mine];
    for _ in 0..4 {
        if !matches!(
            runner.state().waiting_for,
            WaitingFor::TriggerTargetSelection { .. }
        ) {
            break;
        }
        let legal = current_slot_legal(&runner);
        let pick = wanted
            .iter()
            .position(|id| legal.contains(&TargetRef::Object(*id)))
            .map(|i| wanted.remove(i));
        assert!(pick.is_some(), "a wanted graveyard card must be legal");
        choose(&mut runner, pick);
    }
    assert!(wanted.is_empty(), "both graveyard cards were targeted");
    let mut events = Vec::new();
    while !runner.state().stack.is_empty() {
        events.extend(runner.act(GameAction::PassPriority).unwrap().events);
    }
    assert_settled(&runner);

    // CR 701.24a: each owner shuffles their card into their own library, and
    // each of those libraries is shuffled exactly once (APNAP order).
    assert_eq!(shuffle_instruction_players(&events), vec![P0, P1]);
    assert_eq!(runner.state().objects[&their_card].zone, Zone::Library);
    assert!(runner.state().players[1].library.contains(&their_card));
    assert_eq!(runner.state().objects[&reshuffled_mine].zone, Zone::Library);
    assert!(runner.state().players[0].library.contains(&reshuffled_mine));
    assert_eq!(
        runner.state().objects[&my_creature_card].zone,
        Zone::Graveyard,
        "untargeted card stays"
    );
    // CR 714.4: with lore ≥ final chapter and no chapter on the stack, the
    // Saga is sacrificed.
    assert_eq!(runner.state().objects[&saga].zone, Zone::Graveyard);
}

/// The players whose library shuffled, in event order.
fn shuffled_libraries(events: &[GameEvent]) -> Vec<PlayerId> {
    events
        .iter()
        .filter_map(|event| match event {
            GameEvent::PlayerPerformedAction {
                player_id,
                action: PlayerActionKind::ShuffledLibrary,
                ..
            } => Some(*player_id),
            _ => None,
        })
        .collect()
}

/// The players whose library the explicit Shuffle instruction shuffled: the
/// `ShuffledLibrary` events after that instruction's `EffectResolved`. (The
/// preceding move into a library carries the engine's own per-card library
/// randomization, which is not the instruction under test.)
fn shuffle_instruction_players(events: &[GameEvent]) -> Vec<PlayerId> {
    let start = events
        .iter()
        .position(|event| {
            matches!(
                event,
                GameEvent::EffectResolved {
                    kind: EffectKind::Shuffle,
                    ..
                }
            )
        })
        .map_or(events.len(), |i| i + 1);
    events[start..]
        .iter()
        .take_while(|event| {
            !matches!(
                event,
                GameEvent::EffectResolved { kind, .. } if *kind != EffectKind::Shuffle
            )
        })
        .filter_map(|event| match event {
            GameEvent::PlayerPerformedAction {
                player_id,
                action: PlayerActionKind::ShuffledLibrary,
                ..
            } => Some(*player_id),
            _ => None,
        })
        .collect()
}

/// Chapter III in a three-player game: P0's Saga already has two lore counters,
/// so P0's precombat main phase adds the third.
fn scientists_chapter_three_scenario() -> (GameScenario, ObjectId) {
    let mut scenario = GameScenario::new_n_player(3, 42);
    scenario.at_phase(Phase::Upkeep);
    for player in [P0, P1, PlayerId(2)] {
        scenario.with_library_top(player, &["Island"; 8]);
    }
    let saga = scenario
        .add_creature(P0, SCIENTISTS_NAME, 0, 0)
        .as_enchantment()
        .with_subtypes(vec!["Saga"])
        .from_oracle_text(SCIENTISTS_ORACLE)
        .id();
    scenario.with_counter(saga, CounterType::Lore, 2);
    (scenario, saga)
}

/// CR 701.24a + CR 608.2c: "The owners of those cards shuffle them into their
/// libraries" — each distinct owner of a chosen card shuffles their own
/// library exactly once (APNAP order), a player who owns no chosen card does
/// not shuffle, and choosing zero cards shuffles no library.
#[test]
fn scientists_chapter_three_shuffles_each_distinct_owner_once() {
    let p2 = PlayerId(2);
    // (owner of first card, owner of second card, pick?) → expected shufflers.
    let cases: [(PlayerId, PlayerId, bool, Vec<PlayerId>); 4] = [
        (P1, p2, true, vec![P1, p2]),
        (p2, P1, true, vec![P1, p2]),
        (P1, P1, true, vec![P1]),
        (P1, p2, false, vec![]),
    ];
    for (first_owner, second_owner, pick, expected) in cases {
        let (mut scenario, saga) = scientists_chapter_three_scenario();
        let first = scenario
            .add_creature_to_graveyard(first_owner, "First Card", 2, 2)
            .id();
        let second = scenario
            .add_creature_to_graveyard(second_owner, "Second Card", 2, 2)
            .id();
        let mut runner = scenario.build();
        runner.advance_to_phase(Phase::PreCombatMain);
        assert_eq!(lore(&runner, saga), 3);
        pass_until_prompt_or_empty(&mut runner);

        let (chooser, slots) = trigger_target_slots(&runner);
        assert_eq!(chooser, P0);
        assert!(slots[0].optional, "up to two targets");
        if pick {
            for id in [first, second] {
                assert!(current_slot_legal(&runner).contains(&TargetRef::Object(id)));
                choose(&mut runner, Some(id));
            }
        } else {
            choose(&mut runner, None);
        }
        assert!(
            matches!(runner.state().waiting_for, WaitingFor::Priority { .. }),
            "targets announced, got {:?}",
            runner.state().waiting_for
        );
        assert_eq!(runner.state().stack.len(), 1, "chapter III on the stack");

        let mut events = Vec::new();
        for _ in 0..3 {
            events.extend(runner.act(GameAction::PassPriority).unwrap().events);
        }
        assert!(runner.state().stack.is_empty(), "chapter III resolved");

        let case = format!("case ({first_owner:?}, {second_owner:?}, pick={pick})");
        assert_eq!(shuffle_instruction_players(&events), expected, "{case}");
        // No library outside the chosen cards' owners is shuffled at all, and
        // choosing no cards shuffles nothing.
        let shuffled = shuffled_libraries(&events);
        assert!(
            shuffled.iter().all(|p| expected.contains(p)),
            "{case}: {shuffled:?}"
        );
        assert!(!shuffled.contains(&P0), "{case}: P0 owns no chosen card");
        for (id, owner) in [(first, first_owner), (second, second_owner)] {
            let zone = if pick { Zone::Library } else { Zone::Graveyard };
            assert_eq!(runner.state().objects[&id].zone, zone);
            assert_eq!(
                runner.state().players[owner.0 as usize]
                    .library
                    .contains(&id),
                pick,
                "each card goes to its owner's library"
            );
        }
        // CR 714.4: the Saga is sacrificed after its final chapter resolves.
        assert_eq!(runner.state().objects[&saga].zone, Zone::Graveyard);
    }
}

// ---------------------------------------------------------------------------
// New New York (a Plane whose subtype-list animation shares the parser path)
// ---------------------------------------------------------------------------

const NEW_NEW_YORK_ORACLE: &str = "At the beginning of combat on your turn, until end of turn, noncreature artifacts you control become 3/3 Vehicles in addition to their other types and gain flying, haste, and crew 1.\nWhenever chaos ensues, create a Treasure token and a 2/2 white Alien creature token.";

/// CR 208.3 + CR 301.7a + CR 702.122a: "noncreature artifacts you control
/// become 3/3 Vehicles … and gain … crew 1" makes them noncreature Vehicles
/// (no power or toughness while not creatures); crewing one makes it an
/// artifact creature, and only then does the 3/3 apply.
#[test]
fn new_new_york_vehicles_are_not_creatures_until_crewed() {
    let mut scenario = GameScenario::new();
    scenario.at_phase(Phase::PreCombatMain);
    scenario.add_enchantment_from_oracle(P0, "New New York", NEW_NEW_YORK_ORACLE);
    let relic = scenario
        .add_artifact_from_oracle(P0, "Plain Relic", "")
        .id();
    let crewmate = scenario.add_creature(P0, "Crewmate", 2, 2).id();
    let mut runner = scenario.build();

    // Leaving the main phase begins combat, and the trigger fires.
    advance_until(&mut runner, |r| !r.state().stack.is_empty());
    assert_eq!(runner.state().phase, Phase::BeginCombat);
    pass_until_prompt_or_empty(&mut runner);
    assert_eq!(runner.state().phase, Phase::BeginCombat);
    assert!(runner.state().stack.is_empty(), "the trigger resolved");

    {
        let obj = &runner.state().objects[&relic];
        assert!(obj.card_types.subtypes.iter().any(|s| s == "Vehicle"));
        assert!(obj.card_types.core_types.contains(&CoreType::Artifact));
        assert!(
            !obj.card_types.core_types.contains(&CoreType::Creature),
            "a Vehicle is not a creature until crewed"
        );
        // CR 208.3 + CR 208.3a: a noncreature permanent has no power or
        // toughness, but an effect that sets its base P/T is still created and
        // does nothing until it becomes a creature. The engine keeps P/T values
        // on a noncreature `GameObject` (a printed Vehicle's too) rather than
        // clearing them, so the latent 3/3 is recorded here; what CR 208.3
        // requires is the missing Creature type asserted above.
        assert_eq!(
            (obj.power, obj.toughness),
            (Some(3), Some(3)),
            "the setting effect is created while the Vehicle is noncreature"
        );
        assert!(obj.has_keyword(&Keyword::Flying));
        assert!(obj.has_keyword(&Keyword::Haste));
        assert!(obj.has_keyword(&Keyword::Crew {
            power: 1,
            once_per_turn: None,
        }));
    }
    // The crew creature is not a noncreature artifact, so it is unchanged.
    let crew_obj = &runner.state().objects[&crewmate];
    assert!(!crew_obj.card_types.subtypes.iter().any(|s| s == "Vehicle"));

    runner
        .act(GameAction::CrewVehicle {
            vehicle_id: relic,
            creature_ids: vec![],
        })
        .expect("enter crew mode");
    runner
        .act(GameAction::CrewVehicle {
            vehicle_id: relic,
            creature_ids: vec![crewmate],
        })
        .expect("crew 1 with a 2-power creature");
    pass_until_prompt_or_empty(&mut runner);
    assert!(runner.state().stack.is_empty(), "crew resolved");

    let obj = &runner.state().objects[&relic];
    assert!(
        obj.card_types.core_types.contains(&CoreType::Creature),
        "crewed Vehicle is an artifact creature"
    );
    assert_eq!((obj.power, obj.toughness), (Some(3), Some(3)));
    assert!(
        runner.state().objects[&crewmate].tapped,
        "crew taps the crewer"
    );
}

// ---------------------------------------------------------------------------
// Ensign Harry Kim, Operations
// ---------------------------------------------------------------------------

const KIM_NAME: &str = "Ensign Harry Kim, Operations";
const KIM_ORACLE: &str = "Whenever another creature you control enters, if it has greater power than Ensign Kim, put a +1/+1 counter on Ensign Kim.\n{2}, Remove two +1/+1 counters from among creatures you control: Draw a card.";

fn add_kim(scenario: &mut GameScenario) -> ObjectId {
    scenario
        .add_creature(P0, KIM_NAME, 1, 2)
        .as_legendary()
        .with_subtypes(vec!["Human", "Officer"])
        .from_oracle_text(KIM_ORACLE)
        .id()
}

fn plus_one_counters(runner: &GameRunner, id: ObjectId) -> u32 {
    runner.state().objects[&id]
        .counters
        .get(&CounterType::Plus1Plus1)
        .copied()
        .unwrap_or(0)
}

/// CR 201.5c + CR 603.4: "Ensign Kim" is the printed short self-name, so the
/// intervening "if it has greater power than Ensign Kim" compares the entering
/// creature against Kim's own power — a greater-power entrant grows Kim, an
/// equal- or lower-power one does not, and the counter goes on Kim.
#[test]
fn kim_grows_only_when_a_greater_power_creature_enters() {
    let mut scenario = GameScenario::new();
    scenario.at_phase(Phase::PreCombatMain);
    let kim = add_kim(&mut scenario);
    let high = scenario
        .add_creature_to_hand(P0, "Grizzly Bears", 2, 2)
        .with_mana_cost(ManaCost::zero())
        .id();
    let equal = scenario
        .add_creature_to_hand(P0, "Equal Bears", 2, 2)
        .with_mana_cost(ManaCost::zero())
        .id();
    let low = scenario
        .add_creature_to_hand(P0, "Llanowar Elves", 1, 1)
        .with_mana_cost(ManaCost::zero())
        .id();
    let mut runner = scenario.build();

    cast_from_hand(&mut runner, high);
    pass_until_prompt_or_empty(&mut runner);
    assert_settled(&runner);
    assert_eq!(runner.state().objects[&high].zone, Zone::Battlefield);
    assert_eq!(plus_one_counters(&runner, kim), 1, "2 power > Kim's 1");
    assert_eq!(plus_one_counters(&runner, high), 0, "the counter is Kim's");
    assert_eq!(runner.state().objects[&kim].power, Some(2));

    // Kim is now 2/3: neither a 2-power nor a 1-power entrant is greater.
    for card in [equal, low] {
        cast_from_hand(&mut runner, card);
        pass_until_prompt_or_empty(&mut runner);
        assert_settled(&runner);
        assert_eq!(runner.state().objects[&card].zone, Zone::Battlefield);
        assert_eq!(plus_one_counters(&runner, kim), 1);
    }
}

/// CR 602.2b + CR 601.2h: "{2}, Remove two +1/+1 counters from among creatures
/// you control: Draw a card" — the counters may come from two different
/// creatures, and the draw happens once the whole cost is paid.
#[test]
fn kim_activation_removes_counters_from_among_creatures_and_draws() {
    let mut scenario = GameScenario::new();
    scenario.at_phase(Phase::PreCombatMain);
    let kim = add_kim(&mut scenario);
    let donor = scenario.add_creature(P0, "Grizzly Bears", 2, 2).id();
    scenario.with_counter(kim, CounterType::Plus1Plus1, 1);
    scenario.with_counter(donor, CounterType::Plus1Plus1, 1);
    let drawn = scenario
        .add_spell_to_library_top(P0, "Drawn Card", true)
        .id();
    scenario.with_mana_pool(
        P0,
        (0..2)
            .map(|_| ManaUnit::new(ManaType::Colorless, ObjectId(0), false, vec![]))
            .collect(),
    );
    let mut runner = scenario.build();

    runner
        .act(GameAction::ActivateAbility {
            source_id: kim,
            ability_index: 0,
            payment_mode: CastPaymentMode::Auto,
        })
        .expect("activate Kim");
    assert!(
        matches!(
            runner.state().waiting_for,
            WaitingFor::PayCost {
                kind: PayCostKind::RemoveCounter { .. },
                ..
            }
        ),
        "expected the remove-counter cost prompt, got {:?}",
        runner.state().waiting_for
    );
    runner
        .act(GameAction::ChooseRemoveCounterCostDistribution {
            distribution: [kim, donor]
                .into_iter()
                .map(|object_id| CounterCostChoice {
                    object_id,
                    counter_type: CounterType::Plus1Plus1,
                    count: 1,
                })
                .collect(),
        })
        .expect("remove one counter from each");
    // CR 601.2h: the whole cost is paid at activation; nothing is drawn yet.
    assert_eq!(runner.state().objects[&drawn].zone, Zone::Library);
    pass_until_prompt_or_empty(&mut runner);
    assert_settled(&runner);

    assert_eq!(plus_one_counters(&runner, kim), 0);
    assert_eq!(plus_one_counters(&runner, donor), 0);
    assert_eq!(runner.state().objects[&drawn].zone, Zone::Hand);
    assert_eq!(runner.state().players[0].mana_pool.total(), 0);
}

// ---------------------------------------------------------------------------
// Dr. Leonard "Bones" McCoy
// ---------------------------------------------------------------------------

const MCCOY_NAME: &str = "Dr. Leonard \"Bones\" McCoy";
const MCCOY_ORACLE: &str = "Flash\nWhen Dr. McCoy enters, choose one —\n• Return target non-Doctor permanent card from your graveyard to your hand.\n• Choose up to one other target creature. When it dies this turn, return that card to the battlefield under its owner's control with a +1/+1 counter on it at the beginning of the next end step.";

fn add_mccoy_to_hand(scenario: &mut GameScenario) -> ObjectId {
    scenario
        .add_creature_to_hand(P0, MCCOY_NAME, 3, 2)
        .with_mana_cost(ManaCost::zero())
        .with_subtypes(vec!["Human", "Doctor"])
        .as_legendary()
        .from_oracle_text_with_keywords(&["Flash"], MCCOY_ORACLE)
        .id()
}

/// Seed both libraries so turn-structure advancement never decks a player.
fn seed_libraries(scenario: &mut GameScenario) {
    for i in 0..4 {
        scenario.add_spell_to_library_top(P0, &format!("P0 Library {i}"), true);
        scenario.add_spell_to_library_top(P1, &format!("P1 Library {i}"), true);
    }
}

/// Cast McCoy from hand and stop at its ETB mode choice, asserting the prompt.
fn cast_mccoy_to_mode_choice(runner: &mut GameRunner, mccoy: ObjectId) {
    cast_from_hand(runner, mccoy);
    pass_until_prompt_or_empty(runner);
    // CR 201.5c + CR 603.6a: "When Dr. McCoy enters" names this card.
    // CR 700.2b: the controller chooses the mode as the trigger is put on the stack.
    match &runner.state().waiting_for {
        WaitingFor::AbilityModeChoice {
            player,
            modal,
            source_id,
            ..
        } => {
            assert_eq!(*player, P0);
            assert_eq!(*source_id, mccoy);
            assert_eq!(
                (modal.min_choices, modal.max_choices, modal.mode_count),
                (1, 1, 2),
                "choose one of two modes"
            );
        }
        other => panic!("expected McCoy's ETB mode choice, got {other:?}"),
    }
    assert_eq!(runner.state().objects[&mccoy].zone, Zone::Battlefield);
}

/// Choose mode two and answer its "up to one other target creature" slot.
fn mccoy_mode_two_choose(runner: &mut GameRunner, mccoy: ObjectId, chosen: ObjectId) {
    runner
        .act(GameAction::SelectModes { indices: vec![1] })
        .expect("select the death-watch mode");
    let (chooser, slots) = trigger_target_slots(runner);
    assert_eq!(chooser, P0);
    assert_eq!(slots.len(), 1);
    assert!(slots[0].optional, "up to one target is optional");
    let legal = current_slot_legal(runner);
    assert!(legal.contains(&TargetRef::Object(chosen)));
    // "other target creature": McCoy cannot choose itself.
    assert!(!legal.contains(&TargetRef::Object(mccoy)));
    choose(runner, Some(chosen));
    pass_until_prompt_or_empty(runner);
    assert_settled(runner);
    // CR 603.7a: resolving the mode creates the death-watch delayed trigger.
    assert_eq!(
        runner.state().delayed_triggers.len(),
        1,
        "mode two installs exactly one delayed trigger"
    );
}

/// Cast a Lightning Bolt (3 damage) at `victim` through the spell pipeline.
fn bolt(runner: &mut GameRunner, bolt: ObjectId, victim: ObjectId) {
    runner.cast(bolt).target_objects(&[victim]).resolve();
    pass_until_prompt_or_empty(runner);
}

fn advance_past_turn(runner: &mut GameRunner) {
    let start_turn = runner.state().turn_number;
    advance_until(runner, |r| r.state().turn_number > start_turn);
}

#[test]
fn mccoy_mode_two_returns_the_died_card_under_its_owners_control_at_end_step() {
    let mut scenario = GameScenario::new();
    // CR 702.8a: Flash lets McCoy be cast outside a main phase — here at the
    // beginning of combat on its controller's turn.
    scenario.at_phase(Phase::BeginCombat);
    seed_libraries(&mut scenario);
    let mccoy = add_mccoy_to_hand(&mut scenario);
    let patient = scenario.add_creature(P1, "Red Shirt", 2, 2).id();
    let bolt_id = scenario.add_bolt_to_hand(P0);
    let mut runner = scenario.build();

    cast_mccoy_to_mode_choice(&mut runner, mccoy);
    mccoy_mode_two_choose(&mut runner, mccoy, patient);

    bolt(&mut runner, bolt_id, patient);
    // CR 700.4: the chosen creature died. CR 603.7a: the death trigger has
    // resolved and created the end-step return, so the card waits in the
    // graveyard until then.
    assert_eq!(runner.state().objects[&patient].zone, Zone::Graveyard);
    assert_eq!(runner.state().phase, Phase::BeginCombat);

    advance_past_turn(&mut runner);

    // CR 603.7 + CR 400.7: "that card" is the card that died; it returns at
    // the beginning of the next end step. CR 110.2: under its OWNER's control
    // (P1), not the delayed trigger's controller (P0). CR 122.1: with a
    // +1/+1 counter on it.
    let returned = &runner.state().objects[&patient];
    assert_eq!(returned.zone, Zone::Battlefield);
    assert_eq!(returned.controller, P1);
    assert_eq!(plus_one_counters(&runner, patient), 1);
    assert_eq!(runner.state().objects[&mccoy].zone, Zone::Battlefield);
    assert!(
        runner.state().delayed_triggers.is_empty(),
        "both delayed triggers are spent"
    );
}

#[test]
fn mccoy_mode_two_returns_your_own_creature_too() {
    let mut scenario = GameScenario::new();
    scenario.at_phase(Phase::PreCombatMain);
    seed_libraries(&mut scenario);
    let mccoy = add_mccoy_to_hand(&mut scenario);
    let ensign = scenario.add_creature(P0, "Ensign", 1, 1).id();
    let bolt_id = scenario.add_bolt_to_hand(P0);
    let mut runner = scenario.build();

    cast_mccoy_to_mode_choice(&mut runner, mccoy);
    mccoy_mode_two_choose(&mut runner, mccoy, ensign);
    bolt(&mut runner, bolt_id, ensign);
    assert_eq!(runner.state().objects[&ensign].zone, Zone::Graveyard);

    advance_past_turn(&mut runner);

    let returned = &runner.state().objects[&ensign];
    assert_eq!(returned.zone, Zone::Battlefield);
    assert_eq!(returned.controller, P0);
    assert_eq!(plus_one_counters(&runner, ensign), 1);
}

#[test]
fn mccoy_mode_two_returns_nothing_when_the_chosen_creature_survives() {
    let mut scenario = GameScenario::new();
    scenario.at_phase(Phase::PreCombatMain);
    seed_libraries(&mut scenario);
    let mccoy = add_mccoy_to_hand(&mut scenario);
    let patient = scenario.add_creature(P1, "Red Shirt", 2, 2).id();
    let bolt_id = scenario.add_bolt_to_hand(P0);
    let mut runner = scenario.build();

    cast_mccoy_to_mode_choice(&mut runner, mccoy);
    mccoy_mode_two_choose(&mut runner, mccoy, patient);
    // McCoy itself dies instead. The delayed trigger watches the CHOSEN
    // creature (CR 608.2c: "it" is the target just chosen), not its source.
    bolt(&mut runner, bolt_id, mccoy);
    assert_eq!(runner.state().objects[&mccoy].zone, Zone::Graveyard);

    advance_past_turn(&mut runner);

    assert_eq!(
        runner.state().objects[&mccoy].zone,
        Zone::Graveyard,
        "McCoy's own death does not fire the watch"
    );
    let patient_obj = &runner.state().objects[&patient];
    assert_eq!(patient_obj.zone, Zone::Battlefield);
    assert_eq!(plus_one_counters(&runner, patient), 0);
}

/// CR 603.7b: "When it dies this turn" has a stated duration, so an unfired
/// watch expires at the end of the turn that created it and a death on a later
/// turn returns nothing.
#[test]
fn mccoy_mode_two_does_not_return_a_creature_that_dies_on_a_later_turn() {
    let mut scenario = GameScenario::new();
    scenario.at_phase(Phase::PreCombatMain);
    seed_libraries(&mut scenario);
    let mccoy = add_mccoy_to_hand(&mut scenario);
    let patient = scenario.add_creature(P1, "Red Shirt", 2, 2).id();
    let their_bolt = scenario.add_bolt_to_hand(P1);
    let mut runner = scenario.build();

    cast_mccoy_to_mode_choice(&mut runner, mccoy);
    mccoy_mode_two_choose(&mut runner, mccoy, patient);

    // CR 603.7b: the watch has a stated duration of "this turn".
    advance_to_next_main_of(&mut runner, P1);
    assert!(
        runner.state().delayed_triggers.is_empty(),
        "the watch expired at the end of the turn it was created"
    );
    bolt(&mut runner, their_bolt, patient);
    assert_eq!(runner.state().objects[&patient].zone, Zone::Graveyard);

    advance_past_turn(&mut runner);

    assert_eq!(
        runner.state().objects[&patient].zone,
        Zone::Graveyard,
        "a death on a later turn returns nothing"
    );
}

/// CR 601.2c + CR 603.7c: "up to one" lets mode two choose no creature. The
/// watch then refers to nothing and must not be installed — it must never
/// widen to every creature — so another creature dying this turn returns
/// nothing.
#[test]
fn mccoy_mode_two_with_no_target_chosen_returns_nothing() {
    let mut scenario = GameScenario::new();
    scenario.at_phase(Phase::PreCombatMain);
    seed_libraries(&mut scenario);
    let mccoy = add_mccoy_to_hand(&mut scenario);
    let bystander = scenario.add_creature(P1, "Red Shirt", 2, 2).id();
    let bolt_id = scenario.add_bolt_to_hand(P0);
    let mut runner = scenario.build();

    cast_mccoy_to_mode_choice(&mut runner, mccoy);
    runner
        .act(GameAction::SelectModes { indices: vec![1] })
        .expect("select the death-watch mode");
    let (_, slots) = trigger_target_slots(&runner);
    assert!(slots[0].optional, "up to one target is optional");
    assert!(current_slot_legal(&runner).contains(&TargetRef::Object(bystander)));
    choose(&mut runner, None);
    pass_until_prompt_or_empty(&mut runner);
    assert_settled(&runner);
    assert!(
        runner.state().delayed_triggers.is_empty(),
        "with no creature chosen, no death watch is installed"
    );

    bolt(&mut runner, bolt_id, bystander);
    assert_eq!(runner.state().objects[&bystander].zone, Zone::Graveyard);

    advance_past_turn(&mut runner);

    assert_eq!(
        runner.state().objects[&bystander].zone,
        Zone::Graveyard,
        "an unchosen creature's death returns nothing"
    );
    assert_eq!(plus_one_counters(&runner, bystander), 0);
}

#[test]
fn mccoy_mode_one_returns_a_non_doctor_permanent_card_only() {
    let mut scenario = GameScenario::new();
    scenario.at_phase(Phase::PreCombatMain);
    let mccoy = add_mccoy_to_hand(&mut scenario);
    let crewman = scenario
        .add_creature_to_graveyard(P0, "Fallen Crewman", 2, 2)
        .id();
    let land = scenario.add_land_to_graveyard(P0, "Razed Outpost").id();
    let doctor = scenario
        .add_creature_to_graveyard(P0, "Fallen Doctor", 2, 2)
        .with_subtypes(vec!["Human", "Doctor"])
        .id();
    let instant = scenario
        .add_spell_to_graveyard(P0, "Spent Order", true)
        .id();
    let theirs = scenario
        .add_creature_to_graveyard(P1, "Fallen Rival", 2, 2)
        .id();
    let mut runner = scenario.build();

    cast_mccoy_to_mode_choice(&mut runner, mccoy);
    runner
        .act(GameAction::SelectModes { indices: vec![0] })
        .expect("select the return mode");

    let (chooser, slots) = trigger_target_slots(&runner);
    assert_eq!(chooser, P0);
    assert_eq!(slots.len(), 1);
    let legal = current_slot_legal(&runner);
    // CR 115.1: only a non-Doctor permanent card in YOUR graveyard is legal.
    assert!(legal.contains(&TargetRef::Object(crewman)));
    assert!(
        legal.contains(&TargetRef::Object(land)),
        "a land is a permanent card"
    );
    assert!(
        !legal.contains(&TargetRef::Object(doctor)),
        "Doctor excluded"
    );
    assert!(
        !legal.contains(&TargetRef::Object(instant)),
        "nonpermanent excluded"
    );
    assert!(
        !legal.contains(&TargetRef::Object(theirs)),
        "opponent's graveyard excluded"
    );
    choose(&mut runner, Some(crewman));
    pass_until_prompt_or_empty(&mut runner);

    assert_settled(&runner);
    assert_eq!(runner.state().objects[&crewman].zone, Zone::Hand);
    assert_eq!(runner.state().objects[&land].zone, Zone::Graveyard);
    assert_eq!(runner.state().objects[&doctor].zone, Zone::Graveyard);
    assert!(
        runner.state().delayed_triggers.is_empty(),
        "mode one creates no delayed trigger"
    );
}

// ---------------------------------------------------------------------------
// Desperate Measures — same "When it dies … this turn" pronoun class
// ---------------------------------------------------------------------------

const DESPERATE_MEASURES_NAME: &str = "Desperate Measures";
const DESPERATE_MEASURES_ORACLE: &str = "Target creature gets +1/-1 until end of turn. When it dies under your control this turn, draw two cards.";

fn desperate_measures_scenario(victim_controller: PlayerId) -> (GameRunner, ObjectId, ObjectId) {
    let mut scenario = GameScenario::new();
    scenario.at_phase(Phase::PreCombatMain);
    seed_libraries(&mut scenario);
    let spell = scenario
        .add_spell_to_hand_from_oracle(P0, DESPERATE_MEASURES_NAME, true, DESPERATE_MEASURES_ORACLE)
        .id();
    let victim = scenario
        .add_creature(victim_controller, "Frail Ensign", 1, 1)
        .id();
    (scenario.build(), spell, victim)
}

#[test]
fn desperate_measures_draws_two_when_the_target_dies_under_your_control() {
    let (mut runner, spell, victim) = desperate_measures_scenario(P0);
    let outcome = runner.cast(spell).target_objects(&[victim]).resolve();
    // CR 704.5f: +1/-1 leaves the 1/1 with 0 toughness, so it dies.
    // CR 608.2c: "it" is the target creature, not the spell.
    outcome.assert_zone(&[victim], Zone::Graveyard);
    pass_until_prompt_or_empty(&mut runner);
    assert_settled(&runner);
    assert_eq!(runner.state().players[0].hand.len(), 2, "drew two cards");
}

#[test]
fn desperate_measures_draws_nothing_when_the_target_dies_under_an_opponent() {
    let (mut runner, spell, victim) = desperate_measures_scenario(P1);
    let outcome = runner.cast(spell).target_objects(&[victim]).resolve();
    outcome.assert_zone(&[victim], Zone::Graveyard);
    pass_until_prompt_or_empty(&mut runner);
    assert_settled(&runner);
    assert_eq!(
        runner.state().players[0].hand.len(),
        0,
        "the creature died under its controller P1, not under yours"
    );
}

// ---------------------------------------------------------------------------
// Turn Inside Out — same "When it dies this turn" pronoun and duration class
// ---------------------------------------------------------------------------

const TURN_INSIDE_OUT_NAME: &str = "Turn Inside Out";
const TURN_INSIDE_OUT_ORACLE: &str = "Target creature gets +3/+0 until end of turn. When it dies this turn, manifest dread. (Look at the top two cards of your library. Put one onto the battlefield face down as a 2/2 creature and the other into your graveyard. Turn it face up any time for its mana cost if it's a creature card.)";

/// Turn Inside Out in P0's hand aimed at P1's `target`, with a second P1
/// creature and a Lightning Bolt for each player.
struct TurnInsideOutTable {
    runner: GameRunner,
    spell: ObjectId,
    target: ObjectId,
    other: ObjectId,
    our_bolt: ObjectId,
    their_bolt: ObjectId,
}

fn turn_inside_out_table() -> TurnInsideOutTable {
    let mut scenario = GameScenario::new();
    scenario.at_phase(Phase::PreCombatMain);
    seed_libraries(&mut scenario);
    let spell = scenario
        .add_spell_to_hand_from_oracle(P0, TURN_INSIDE_OUT_NAME, true, TURN_INSIDE_OUT_ORACLE)
        .id();
    let target = scenario.add_creature(P1, "Pumped Ensign", 2, 2).id();
    let other = scenario.add_creature(P1, "Bystander", 2, 2).id();
    let our_bolt = scenario.add_bolt_to_hand(P0);
    let their_bolt = scenario.add_bolt_to_hand(P1);
    TurnInsideOutTable {
        runner: scenario.build(),
        spell,
        target,
        other,
        our_bolt,
        their_bolt,
    }
}

fn battlefield_face_down(runner: &GameRunner) -> Vec<ObjectId> {
    runner
        .state()
        .battlefield
        .iter()
        .copied()
        .filter(|id| runner.state().objects[id].face_down)
        .collect()
}

#[test]
fn turn_inside_out_manifests_dread_when_the_target_dies_this_turn() {
    let TurnInsideOutTable {
        mut runner,
        spell,
        target,
        our_bolt,
        ..
    } = turn_inside_out_table();
    runner.cast(spell).target_objects(&[target]).resolve();
    pass_until_prompt_or_empty(&mut runner);
    assert_settled(&runner);
    assert_eq!(
        runner.state().delayed_triggers.len(),
        1,
        "the spell installs the death watch"
    );

    // CR 608.2c: "it" is the target creature. CR 700.4: it dies this turn.
    bolt(&mut runner, our_bolt, target);
    assert_eq!(runner.state().objects[&target].zone, Zone::Graveyard);

    // CR 701.62a: manifest dread — the controller chooses one of the top two.
    let WaitingFor::ManifestDreadChoice { player, cards, .. } = runner.state().waiting_for.clone()
    else {
        panic!(
            "the death watch must fire manifest dread, got {:?}",
            runner.state().waiting_for
        );
    };
    assert_eq!(player, P0);
    assert_eq!(cards.len(), 2);
    runner
        .act(GameAction::SelectCards {
            cards: vec![cards[0]],
        })
        .expect("choose the card to manifest");
    pass_until_prompt_or_empty(&mut runner);
    assert_settled(&runner);

    assert_eq!(battlefield_face_down(&runner), vec![cards[0]]);
    assert_eq!(runner.state().objects[&cards[0]].controller, P0);
    assert_eq!(runner.state().objects[&cards[1]].zone, Zone::Graveyard);
    assert!(runner.state().delayed_triggers.is_empty());
}

#[test]
fn turn_inside_out_ignores_other_deaths_and_deaths_on_a_later_turn() {
    let TurnInsideOutTable {
        mut runner,
        spell,
        target,
        other,
        our_bolt,
        their_bolt,
    } = turn_inside_out_table();
    let library_before = runner.state().players[0].library.len();
    runner.cast(spell).target_objects(&[target]).resolve();
    pass_until_prompt_or_empty(&mut runner);
    assert_settled(&runner);

    // CR 608.2c: the watch is bound to the target, not to any creature.
    bolt(&mut runner, our_bolt, other);
    assert_eq!(runner.state().objects[&other].zone, Zone::Graveyard);
    assert_settled(&runner);
    assert_eq!(
        runner.state().delayed_triggers.len(),
        1,
        "another creature's death neither fires nor spends the watch"
    );

    // CR 603.7b: "this turn" — the unfired watch expires with the turn.
    advance_to_next_main_of(&mut runner, P1);
    assert!(
        runner.state().delayed_triggers.is_empty(),
        "the watch expired at the end of the turn it was created"
    );
    bolt(&mut runner, their_bolt, target);
    assert_eq!(runner.state().objects[&target].zone, Zone::Graveyard);
    assert_settled(&runner);

    assert!(
        battlefield_face_down(&runner).is_empty(),
        "nothing manifested"
    );
    // Manifest dread never looked at P0's library (only P1 drew since).
    assert_eq!(runner.state().players[0].library.len(), library_before);
}
