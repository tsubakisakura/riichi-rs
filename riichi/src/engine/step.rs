use crate::{
    analysis::IrregularWait,
    common::*,
    engine::distribute_points,
    model::*
};
use super::{
    utils::*,
    EngineCache,
    RIICHI_POT
};

/// Process normal end-of-turn flow (no abort, no win).
/// Each change to the state is processed in chronological order, gradually morphing the current
/// state to the next. This avoids copying the entire state.
pub(crate) fn next_normal(
    begin: &RoundBegin,
    state: &State,
    action: Action,
    action_result: ActionResult,
    cache: &EngineCache,
) -> StateCore {
    let mut next = state.core;
    let actor = state.core.action_player;
    let actor_i = actor.to_usize();

    // Special case: Deferred revealing of new dora indicators due to Kakan/Daiminkan.
    // Why this is special:
    //
    // - **Timing**: The player who did a Kakan/Daiminkan has finished their turn after drawing
    //   from the tail of the wall. Revealing now makes sure that the player did not know what
    //   the new dora indicator is right after making the call.
    //
    // - **Information**: This solely relies on the result of the previous turn. Actions and
    //   reactions during this turn has no effect on this.
    //
    // TODO(summivox): rules (kan-dora)
    if let Some(Meld::Kakan(_)) | Some(Meld::Daiminkan(_)) = state.core.incoming_meld {
        next.num_dora_indicators += 1;
    }

    next.seq += 1;

    // Commit the action.
    // Note that the round has not ended. This means if there's an reaction, it must be a call
    // (Chii/Pon/Daiminkan) on this turn's Discard. Therefore we can merge reaction handling
    // into discard handling.
    match action {
        Action::Discard(discard) => {
            let caller =
                if let ActionResult::CalledBy(caller) = action_result { caller } else { actor };

            // Handle both existing and new riichi.
            if state.core.riichi[actor_i].is_active {
                // Ippatsu naturally expires after the first discard since declaring riichi.
                next.riichi[actor_i].is_ippatsu = false;
            } else if discard.declares_riichi {
                // Round has not ended => the new riichi is successful.
                next.riichi[actor_i] = RiichiFlags {
                    is_active: true,
                    is_ippatsu: caller == actor,  // no ippatsu if immediately called
                    is_double: is_first_chance(state),
                }
            }

            if caller == actor {
                // No one called. Next turn is the next player (surprise!).
                next.action_player = player_succ(actor);
                next.incoming_meld = None;
                next.draw = Some(begin.wall[state.core.num_drawn_head as usize]);
                next.num_drawn_head += 1;
            } else {
                // Someone called and will take the next turn instead.
                let meld = cache.meld[caller.to_usize()].unwrap();

                next.action_player = caller;
                next.incoming_meld = Some(meld);
                if meld.is_kan() {
                    next.draw = Some(wall::kan_draw(&begin.wall, state.core.num_drawn_tail as usize));
                    next.num_drawn_tail += 1;
                } else {
                    next.draw = None
                }
            }

            // Check Furiten status for the discarding player.
            // furiten-by-discard == some tile in the waiting set exists in the discard set
            if !state.core.furiten[actor_i].miss_permanent {
                let discard_set = TileMask34::from_iter(
                    state.discards[actor_i].iter().map(|discard| discard.tile));
                let waiting_set = cache.wait[actor_i].waiting_set;

                next.furiten[actor_i].by_discard = discard_set.0 & waiting_set.0 > 0;

                // TODO DEBUG
                /*
                if discard_set.0 & waiting_set.0 > 0 {
                    println!("P{} discard furiten", actor_i);
                    println!("discard:{}", discard_set);
                    println!("waiting:{}", waiting_set);
                    for w in cache.wait[actor_i].regular.iter() {
                        println!("{}", w);
                    }
                    println!("-------");
                }
                 */
            }
            // Temporary miss expires after discarding.
            next.furiten[actor_i].miss_temporary = false;

        }

        Action::Ankan(_) | Action::Kakan(_) => {
            // The current player has made an Ankan/Kakan and is entitled to a bonus turn.
            // The round has not ended => no reaction is possible on this.
            let ankan_or_kakan = cache.meld[actor_i].unwrap();

            next.action_player = actor;
            next.incoming_meld = Some(ankan_or_kakan);
            next.draw = Some(wall::kan_draw(&begin.wall, state.core.num_drawn_tail as usize));
            next.num_drawn_tail += 1;

            // Only for Ankan: reveal the next dora indicator immediately.
            // For Kakan, it will only be revealed at the end of the next turn, in the same way
            // as Daiminkan (see above).
            // TODO(summivox): rules (kan-dora)
            if let Action::Ankan(_) = action {
                next.num_dora_indicators += 1;
            }
        }

        Action::TsumoAgari(_) | Action::AbortNineKinds => panic!()
    }

    // Any kind of meld will forcefully break any active riichi ippatsu.
    if next.incoming_meld.is_some() {
        for player in all_players() {
            next.riichi[player.to_usize()].is_ippatsu = false;
        }
    }

    // Check Furiten status for other players.
    // For another player who misses the chance to win (discard in waiting set):
    // - Immediately enters temporary miss state
    // - Immediately enters permanent miss state if under riichi
    // TODO(summivox): ankan should only affect kokushi-tenpai here, although kakan is treated
    //     the same as Ron.
    // TODO(summivox): rules (kokushi-ankan)
    let action_tile = action.tile().unwrap();
    for other_player in other_players_after(actor) {
        let other_player_i = other_player.to_usize();
        let furiten = &mut next.furiten[other_player_i];

        if furiten.miss_permanent { continue; }
        if let Action::Ankan(_) = action {
            if matches!(cache.wait[other_player_i].irregular,
                Some(IrregularWait::ThirteenOrphans(_)) |
                Some(IrregularWait::ThirteenOrphansAll)) {
                furiten.miss_temporary = true;
                furiten.miss_permanent = state.core.riichi[other_player_i].is_active;
            }
        } else {
            if cache.wait[other_player_i].waiting_set.has(action_tile) {
                furiten.miss_temporary = true;
                furiten.miss_permanent = state.core.riichi[other_player_i].is_active;
            }
        }
    }

    next
}

pub(crate) fn next_agari(
    begin: &RoundBegin,
    state: &State,
    action: Action,
    reactions: &[Option<Reaction>; 4],
    agari_kind: AgariKind,
    cache: &EngineCache,
) -> RoundEnd {
    let mut agari_result: [Option<AgariResult>; 4] = [None, None, None, None];
    let mut delta = [0; 4];
    let mut extra_dora_indicator = 0;

    // Workaround for a corner case:
    // 1. Kakan/Daiminkan
    // 2. Draw from the tail of the wall
    // 3. Discard
    // 4. Ron
    //
    // #2, #3, #4 are in the same turn. However, due to the Ron, we haven't triggered the delayed
    // reveaing logic in [`next_normal`], and instead got here. But the discard did happen!
    // To make up, we must reveal one more dora indicator, but only for Ron.
    if let Some(Meld::Kakan(_)) | Some(Meld::Daiminkan(_)) = state.core.incoming_meld {
        extra_dora_indicator = 1;
    }

    match agari_kind {
        AgariKind::Tsumo => {
            let winner = state.core.action_player;
            let winning_tile = state.core.draw.unwrap();
            let agari_result_one = finalize_agari(
                begin, state, cache, agari_kind,
                true, 0,
                winner, winner, winning_tile);
            delta = agari_result_one.points_delta;
            agari_result[winner.to_usize()] = Some(agari_result_one);
        }

        AgariKind::Ron => {
            // TODO(summivox): rules (atama-hane)
            let contributor = state.core.action_player;
            let winning_tile = action.tile().unwrap();
            let mut take_pot = true;
            for winner in other_players_after(contributor) {
                if let Some(Reaction::RonAgari) = reactions[winner.to_usize()] {
                    let agari_result_one = finalize_agari(
                        begin, state, cache, agari_kind,
                        take_pot, extra_dora_indicator,
                        winner, contributor, winning_tile);
                    for i in 0..4 { delta[i] += agari_result_one.points_delta[i]; }
                    agari_result[winner.to_usize()] = Some(agari_result_one);
                    take_pot = false;
                }
            }
        }
    }

    let mut points = begin.points;
    for i in 0..4 { points[i] += delta[i]; }
    let renchan = agari_result[begin.round_id.button().to_usize()].is_some();

    // TODO(summivox): entire game termination (for now assume game will keep going on)
    let next_round_id = if renchan {
        Some(begin.round_id.next_honba(true))
    } else {
        Some(begin.round_id.next_kyoku())
    };

    RoundEnd {
        round_result: ActionResult::Agari(agari_kind),
        pot: 0,
        points,
        points_delta: delta,
        renchan,
        next_round_id,
        agari_result,
    }
}

fn finalize_agari(
    begin: &RoundBegin,
    state: &State,
    cache: &EngineCache,
    agari_kind: AgariKind,
    take_pot: bool,
    extra_dora_indicator: u8,
    winner: Player,
    contributor: Player,
    winning_tile: Tile,
) -> AgariResult {
    let winner_i = winner.to_usize();
    let all_tiles = get_all_tiles(
        agari_kind,
        &state.closed_hands[winner_i],
        winning_tile,
        &state.melds[winner_i],
    );
    let dora_hits = count_doras(
        &all_tiles,
        state.core.num_dora_indicators + extra_dora_indicator,
        &begin.wall,
        state.core.riichi[winner_i].is_active,
    );
    let candidates = &cache.win[winner.to_usize()];
    let mut best_candidate = candidates.iter().max_by_key(|candidate| {
        (Scoring { dora_hits, ..candidate.scoring }).basic_points()
    }).unwrap().clone();
    best_candidate.scoring.dora_hits = dora_hits;
    let mut delta = distribute_points(
        &begin.rules,
        begin.round_id,
        take_pot,
        winner,
        contributor,
        best_candidate.scoring.basic_points(),
    );
    if take_pot {
        delta[winner_i] += begin.pot + RIICHI_POT * num_active_riichi(state) as GamePoints;
    }
    AgariResult {
        winner,
        contributor,
        liable_player: winner,  // TODO(summivox): rules (pao)
        points_delta: delta,
        details: best_candidate,
    }
}

pub(crate) fn next_abort(
    begin: &RoundBegin,
    state: &State,
    abort_reason: AbortReason,
    cache: &EngineCache,
) -> RoundEnd {
    let mut end = RoundEnd {
        round_result: ActionResult::Abort(abort_reason),
        pot: begin.pot + (num_active_riichi(state) as GamePoints * RIICHI_POT),
        points: begin.points,
        ..RoundEnd::default()
    };

    let round_id = begin.round_id;
    let button = round_id.button();
    // ugly syntax gets around array::map moving the Vec value
    let waiting = [0, 1, 2, 3].map(|i| cache.wait[i].waiting_set.any() as u8);
    let waiting_renchan = waiting[button.to_usize()] > 0;
    match abort_reason {
        AbortReason::WallExhausted => {
            end.points_delta = calc_wall_exhausted_delta(waiting);
            end.renchan = waiting_renchan;
            end.next_round_id = Some(round_id.next_honba(waiting_renchan));
        }
        AbortReason::NagashiMangan => {
            end.points_delta = calc_nagashi_mangan_delta(state, button);
            end.renchan = waiting_renchan;
            end.next_round_id = Some(round_id.next_honba(waiting_renchan));
        }

        AbortReason::NineKinds | AbortReason::FourKan | AbortReason::FourWind |
        AbortReason::FourRiichi | AbortReason::DoubleRon | AbortReason::TripleRon => {
            // force renchan with honba + 1
            end.renchan = true;
            end.next_round_id = Some(round_id.next_honba(true));
        }
    }

    for i in 0..4 { end.points[i] += end.points_delta[i]; }

    end
}
