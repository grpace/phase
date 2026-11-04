//! Star Trek (TRK) cards unlocked by three shared parser fixes, proved through
//! the live cast → trigger → resolve pipeline with verbatim MTGJSON Oracle text:
//!
//! - Commander Raffi Musiker — printed short self-name "Commander Musiker"
//!   (CR 201.5c) on a modal ETB trigger.
//! - Worf, Intelligence Contractor — plural registered subtype "Warriors"
//!   becoming the `Warrior` creature type (CR 205.3m / CR 205.1b).
//! - Those Old Scientists — Saga whose reminder text spans a line break
//!   (MTGJSON prints "after your draw\nstep"), so chapter headers must still
//!   be recognized (CR 714.2b).
//!
//! Every test builds its card from the verbatim Oracle text through
//! `from_oracle_text`, so it exercises the same parser the card-data export
//! uses; reverting any of the three fixes changes the parse these tests drive.

use engine::game::scenario::{GameRunner, GameScenario, P0, P1};
use engine::game::triggers::drain_order_triggers_with_identity;
use engine::types::ability::TargetRef;
use engine::types::actions::GameAction;
use engine::types::card_type::CoreType;
use engine::types::counter::CounterType;
use engine::types::game_state::{CastPaymentMode, TargetSelectionSlot, WaitingFor};
use engine::types::identifiers::ObjectId;
use engine::types::keywords::Keyword;
use engine::types::mana::ManaCost;
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

/// Answer Worf's trigger target prompt. Object slots take the next `picks`
/// creature that is legal there; any player slot the engine surfaces for
/// "that player" takes the controller of the next unpicked creature (and must
/// only ever offer opponents). Returns each object slot's legal targets.
fn drive_worf_targets(runner: &mut GameRunner, picks: &[ObjectId]) -> Vec<Vec<TargetRef>> {
    let (chooser, _) = trigger_target_slots(runner);
    assert_eq!(chooser, P0);
    let mut remaining: Vec<ObjectId> = picks.to_vec();
    let mut object_slots = Vec::new();
    for _ in 0..16 {
        if !matches!(
            runner.state().waiting_for,
            WaitingFor::TriggerTargetSelection { .. }
        ) {
            break;
        }
        let legal = current_slot_legal(runner);
        let player_slot = legal.iter().all(|t| matches!(t, TargetRef::Player(_)));
        if player_slot && !legal.is_empty() {
            assert!(
                !legal.contains(&TargetRef::Player(P0)),
                "only opponents may be chosen as \"that player\": {legal:?}"
            );
            let next_controller = remaining
                .first()
                .map(|id| TargetRef::Player(runner.state().objects[id].controller));
            let pick = next_controller
                .filter(|p| legal.contains(p))
                .unwrap_or_else(|| legal[0].clone());
            runner
                .act(GameAction::ChooseTarget { target: Some(pick) })
                .expect("player slot accepted");
        } else {
            let pick = remaining
                .iter()
                .position(|id| legal.contains(&TargetRef::Object(*id)))
                .map(|i| remaining.remove(i));
            object_slots.push(legal);
            choose(runner, pick);
        }
    }
    assert!(
        remaining.is_empty(),
        "every intended creature must be targetable; unpicked: {remaining:?}"
    );
    object_slots
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
    assert_eq!(creature_slots.len(), 1, "one creature slot per opponent");
    let legal = &creature_slots[0];
    assert!(legal.contains(&TargetRef::Object(small)), "MV 3 is legal");
    assert!(
        !legal.contains(&TargetRef::Object(big)),
        "MV 4 is not legal"
    );
    assert!(
        !legal.contains(&TargetRef::Object(mine)),
        "own creature is not legal"
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
/// that player controls" — with two opponents Worf can take one creature from
/// each of them.
#[test]
fn worf_takes_one_creature_from_each_opponent() {
    let mut scenario = GameScenario::new_n_player(3, 42);
    scenario.at_phase(Phase::PreCombatMain);
    let p2 = PlayerId(2);
    let worf = scenario
        .add_creature_to_hand_from_oracle(P0, WORF_NAME, 3, 2, WORF_ORACLE)
        .with_mana_cost(ManaCost::zero())
        .id();
    let from_p1 = scenario
        .add_creature(P1, "Left Bear", 2, 2)
        .with_mana_cost(ManaCost::generic(2))
        .id();
    let from_p2 = scenario
        .add_creature(p2, "Right Bear", 2, 2)
        .with_mana_cost(ManaCost::generic(2))
        .id();
    let mut runner = scenario.build();

    cast_from_hand(&mut runner, worf);
    pass_until_prompt_or_empty(&mut runner);
    drive_worf_targets(&mut runner, &[from_p1, from_p2]);
    pass_until_prompt_or_empty(&mut runner);
    assert_settled(&runner);

    for id in [from_p1, from_p2] {
        let obj = &runner.state().objects[&id];
        assert_eq!(obj.controller, P0, "{} was stolen", obj.name);
        assert!(obj.card_types.subtypes.iter().any(|s| s == "Warrior"));
        assert!(obj.has_keyword(&Keyword::Haste));
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
    pass_until_prompt_or_empty(&mut runner);
    assert_settled(&runner);

    // CR 701.24a: each owner shuffles their card into their own library.
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
