use itertools::Itertools;
use crate::analysis::RegularWait;

use crate::common::*;
use crate::model::*;

impl TileSet37 {
    pub fn terminal_kinds(&self) -> u8 {
        0u8 + (self[0] > 0) as u8 + (self[8] > 0) as u8
            + (self[9] > 0) as u8 + (self[17] > 0) as u8
            + (self[18] > 0) as u8 + (self[26] > 0) as u8
            + (self[27] > 0) as u8 + (self[28] > 0) as u8
            + (self[29] > 0) as u8 + (self[30] > 0) as u8
            + (self[31] > 0) as u8 + (self[32] > 0) as u8
            + (self[33] > 0) as u8
    }
}

pub fn all_players() -> [Player; 4] {
    [Player::new(0), Player::new(1), Player::new(2), Player::new(3)]
}
pub fn player_succ(player: Player) -> Player { Player::new(1).wrapping_add(player) }
pub fn player_oppo(player: Player) -> Player { Player::new(2).wrapping_add(player) }
pub fn player_pred(player: Player) -> Player { Player::new(3).wrapping_add(player) }
pub fn other_players_after(player: Player) -> [Player; 3] {
    [
        Player::new(1).wrapping_add(player),
        Player::new(2).wrapping_add(player),
        Player::new(3).wrapping_add(player),
    ]
}

/// Returns if this discard immediately after calling Chii/Pon constitutes a swap call (喰い替え),
/// i.e. the discarded tile can form the same group as the meld. This is usually forbidden.
///
/// Example:
/// - Hand 456m; if 56m is used to call 7m, then 4m cannot be discarded.
/// - Hand 678m; if 68m is used to call 7m, then the other 7m in hand cannot be discarded.
///
/// <https://riichi.wiki/Kuikae>
pub fn is_forbidden_swap_call(meld: Meld, discard: Tile) -> bool {
    // TODO(summivox): rules (kuikae)
    let discard = discard.to_normal();
    match meld {
        Meld::Chii(chii) => {
            chii.called.to_normal() == discard ||
                (chii.dir() == 0 && Some(discard) == chii.own[1].succ()) ||
                (chii.dir() == 2 && Some(discard) == chii.min.pred())
        }
        Meld::Pon(pon) => {
            pon.called.to_normal() == discard
        }
        _ => false,
    }
}

/// <https://riichi.wiki/Kan#Kan_during_riichi>
pub fn is_ankan_ok_under_riichi(decomps: &[RegularWait], ankan: Tile) -> bool {
    // TODO(summivox): rules (ankan-riichi, okuri-kan, relaxed-ankan-riichi)
    // TODO(summivox): okuri-kan (need to also check the discard)
    // TODO(summivox): relaxed rule (sufficient to not change the set of waiting tiles)
    let ankan = ankan.to_normal();
    decomps.iter().all(|decomp|
        decomp.groups().any(|group| group == HandGroup::Koutsu(ankan)))
}

/********/

pub fn num_active_riichi(state: &State) -> usize {
    state.riichi.into_iter().filter(|flags| flags.is_active).count()
}

/// Affects [`ActionResult::AbortNineKinds`] and [`RiichiFlags::is_double`].
pub fn is_init_abortable(state: &State) -> bool {
    state.seq <= 3 && state.melds.iter().all(|melds| melds.is_empty())
}

/// Checks if [`ActionResult::AbortWallExhausted`] applies (during end-of-turn resolution).
pub fn is_wall_exhausted(state: &State) -> bool {
    state.num_drawn_head + state.num_drawn_tail == wall::MAX_NUM_DRAWS
}

/// Checks if [`ActionResult::AbortNagashiMangan`] applies (during end-of-turn resolution) for the
/// specified player.
/// Assuming [`is_wall_exhausted`].
pub fn is_nagashi_mangan(state: &State, player: Player) -> bool {
    state.discards[player.to_usize()].iter().all(|discard|
        discard.tile.is_terminal() && discard.called_by == player)
}

/// Checks if [`ActionResult::AbortNagashiMangan`] applies (during end-of-turn resolution) for all
/// players.
/// Assuming [`is_wall_exhausted`].
pub fn is_any_player_nagashi_mangan(state: &State) -> bool {
    all_players().into_iter().all(|player| is_nagashi_mangan(state, player))
}

/// Checks if [`ActionResult::AbortFourWind`] applies (during end-of-turn resolution).
pub fn is_aborted_four_wind(state: &State, action: Action) -> bool {
    if let Action::Discard(discard) = action {
        return is_init_abortable(state) &&
            state.seq == 3 &&
            discard.tile.is_wind() &&
            state.discards[0..3].iter().all(|discards|
                discards.len() == 1 && discards[0].tile == discard.tile);
    }
    false
}

/// Checks if [`ActionResult::AbortFourKan`] applies (during end-of-turn resolution).
pub fn is_aborted_four_kan(state: &State, action: Action, tentative_result: ActionResult) -> bool {
    let pp = state.action_player.to_usize();

    if matches!(action, Action::Kakan(_)) ||
        matches!(action, Action::Ankan(_)) ||
        tentative_result == ActionResult::Daiminkan {
        let kan_players =
            state.melds.iter().enumerate().flat_map(|(player, melds_p)|
                melds_p.iter().filter_map(move |meld|
                    if meld.is_kan() { Some(player) } else { None })).collect_vec();

        if kan_players.len() == 4 ||
            kan_players.len() == 3 && !kan_players.iter().all(|&player| player == pp) {
            return true;
        }
    }
    false
}

/// Checks if [`ActionResult::AbortFourRiichi`] applies (during end-of-turn resolution).
pub fn is_aborted_four_riichi(state: &State, action: Action) -> bool {
    matches!(action, Action::Discard(Discard{declares_riichi: true, ..})) &&
        num_active_riichi(state) == 3  // not a typo --- the last player only declared => not active yet
}

/// When the wall has been exhausted, returns the points delta for each player as well as if the
/// button player stays the same in the next round (renchan 連荘).
pub fn resolve_wall_exhausted(
    state: &State, waiting: [u8; 4], button: Player) -> ([GamePoints; 4], bool) {
    let renchan = waiting[button.to_usize()] > 0;
    let delta_nagashi = calc_nagashi_mangan_delta(state, button);
    if delta_nagashi == [0; 4] {
        (calc_wall_exhausted_delta(waiting), renchan)
    } else {
        (delta_nagashi, renchan)
    }
}

/// When the wall has been exhausted and no player has achieved
/// [`ActionResult::AbortNagashiMangan`], given whether each player is waiting (1) or not (0),
/// returns the points delta for each player.
pub fn calc_wall_exhausted_delta(waiting: [u8; 4]) -> [GamePoints; 4] {
    // TODO(summivox): rules (ten-no-ten points)
    const NO_WAIT_PENALTY_TOTAL: GamePoints = 3000;
    let no_wait = NO_WAIT_PENALTY_TOTAL;

    let num_waiting = waiting.into_iter().sum();
    let (down, up) = match num_waiting {
        1 => (-no_wait / 3, no_wait / 1),
        2 => (-no_wait / 2, no_wait / 2),
        3 => (-no_wait / 1, no_wait / 3),
        _ => (0, 0),
    };
    waiting.map(|w| if w > 0 { up } else { down })
}

/// When the wall has been exhausted and some player has achieved
/// [`ActionResult::AbortNagashiMangan`], returns the points delta for each player.
pub fn calc_nagashi_mangan_delta(state: &State, button: Player) -> [GamePoints; 4] {
    // TODO(summivox): rules (nagashi-mangan-points)

    let mut delta = [0; 4];
    for player in all_players() {
        if is_nagashi_mangan(state, player) {
            if player == button {
                delta[player.to_usize()] += 12000 + 4000;
                for qq in 0..4 { delta[qq] -= 4000; }
            } else {
                delta[player.to_usize()] += 8000 + 2000;
                delta[button.to_usize()] -= 2000;
                for qq in 0..4 { delta[qq] -= 2000; }
            }
        }
    }
    delta
}
