//! CR 400.3 + CR 701.24c: "shuffle them into <library>" moves the named objects
//! into the library before the shuffle — never a bare shuffle that leaves them
//! where they were.

use engine::game::scenario::{GameScenario, P0, P1};
use engine::types::events::{GameEvent, PlayerActionKind};
use engine::types::identifiers::ObjectId;
use engine::types::mana::{ManaType, ManaUnit};
use engine::types::phase::Phase;
use engine::types::player::PlayerId;
use engine::types::zones::Zone;

/// Printed Oracle text of Turn the Earth (MTGJSON AtomicCards).
const TURN_THE_EARTH: &str = "Choose up to three target cards in graveyards. The owners of those cards shuffle them into their libraries. You gain 2 life.\nFlashback {1}{G} (You may cast this card from your graveyard for its flashback cost. Then exile it.)";

fn shuffles_for(events: &[GameEvent], player: PlayerId) -> usize {
    events
        .iter()
        .filter(|event| {
            matches!(
                event,
                GameEvent::PlayerPerformedAction {
                    player_id,
                    action: PlayerActionKind::ShuffledLibrary,
                    ..
                } if *player_id == player
            )
        })
        .count()
}

/// Two targets owned by different players: each goes to ITS OWNER's library,
/// each owner's library is shuffled, and the caster still gains 2 life.
#[test]
fn turn_the_earth_puts_each_target_into_its_owners_library_then_each_owner_shuffles() {
    let mut scenario = GameScenario::new();
    scenario.at_phase(Phase::PreCombatMain);
    let spell = scenario
        .add_spell_to_hand_from_oracle(P0, "Turn the Earth", false, TURN_THE_EARTH)
        .id();
    scenario.with_mana_pool(
        P0,
        vec![
            ManaUnit::new(ManaType::Green, ObjectId(0), false, vec![]),
            ManaUnit::new(ManaType::Colorless, ObjectId(0), false, vec![]),
        ],
    );
    let mine = scenario
        .add_creature_to_graveyard(P0, "P0 Graveyard Bear", 2, 2)
        .id();
    let theirs = scenario
        .add_creature_to_graveyard(P1, "P1 Graveyard Bear", 2, 2)
        .id();
    scenario.add_card_to_library_top(P0, "P0 Library Card");
    scenario.add_card_to_library_top(P1, "P1 Library Card");

    let mut runner = scenario.build();
    let outcome = runner.cast(spell).target_objects(&[mine, theirs]).resolve();
    let state = outcome.state();
    let events = outcome.events();

    assert_eq!(state.objects[&mine].zone, Zone::Library);
    assert!(state.players[P0.0 as usize].library.contains(&mine));
    assert_eq!(state.objects[&theirs].zone, Zone::Library);
    assert!(state.players[P1.0 as usize].library.contains(&theirs));
    // Each owner's library is shuffled. (`ChangeZone` into a library already
    // auto-shuffles on each move and the shared `ChangeZoneToLibrary` lowering
    // adds a terminal owner shuffle — the same pre-existing shape as the
    // Cavalier cycle's "shuffle it into its owner's library" — so this asserts
    // presence, not an exact count.)
    assert!(shuffles_for(events, P0) >= 1, "{events:?}");
    assert!(shuffles_for(events, P1) >= 1, "{events:?}");
    outcome.assert_life_delta(P0, 2);
}

/// CR 115.6 + CR 608.2c + CR 701.24a: choosing zero targets is legal for "up to
/// three". "The owners of those cards" then names no player, so the Shuffle
/// instruction shuffles no library, while "You gain 2 life" — which does not
/// refer to the chosen cards — still happens.
#[test]
fn turn_the_earth_with_zero_targets_shuffles_no_library_and_still_gains_two() {
    let mut scenario = GameScenario::new();
    scenario.at_phase(Phase::PreCombatMain);
    let spell = scenario
        .add_spell_to_hand_from_oracle(P0, "Turn the Earth", false, TURN_THE_EARTH)
        .id();
    scenario.with_mana_pool(
        P0,
        vec![
            ManaUnit::new(ManaType::Green, ObjectId(0), false, vec![]),
            ManaUnit::new(ManaType::Colorless, ObjectId(0), false, vec![]),
        ],
    );
    // A legal target exists in each graveyard, so declining is a real choice.
    let mine = scenario
        .add_creature_to_graveyard(P0, "P0 Graveyard Bear", 2, 2)
        .id();
    let theirs = scenario
        .add_creature_to_graveyard(P1, "P1 Graveyard Bear", 2, 2)
        .id();
    scenario.add_card_to_library_top(P0, "P0 Library Card");
    scenario.add_card_to_library_top(P1, "P1 Library Card");

    let mut runner = scenario.build();
    let outcome = runner.cast(spell).resolve();
    let state = outcome.state();
    let events = outcome.events();

    assert_eq!(
        state.objects[&spell].zone,
        Zone::Graveyard,
        "the spell resolved"
    );
    assert_eq!(state.objects[&mine].zone, Zone::Graveyard);
    assert_eq!(state.objects[&theirs].zone, Zone::Graveyard);
    assert_eq!(shuffles_for(events, P0), 0, "{events:?}");
    assert_eq!(shuffles_for(events, P1), 0, "{events:?}");
    outcome.assert_life_delta(P0, 2);
}
