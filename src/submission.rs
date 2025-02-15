use crate::container::ContainerHandle;
use anyhow::{Context, Result};
use rplcs_events::tournament_1::{
    ChoiceResponse, FightChoices, FightInfo, GambleChoices, MoveChoices, PlayerState,
};

pub struct Submission {
    pub name: String,
    container: ContainerHandle,
    player_state: PlayerState,
}

impl Submission {
    pub fn new(name: &str, container: ContainerHandle) -> Self {
        Submission {
            name: name.to_string(),
            container,
            player_state: PlayerState {
                health: 3,
                max_health: 3,
                power: 5,
            },
        }
    }

    pub async fn get_choices(&self, choices: &MoveChoices, game_id: i64) -> Result<ChoiceResponse> {
        self.container
            .call("choices", game_id, choices)
            .await
            .context("Failed to get choices")
    }

    pub async fn get_gamble_choice(&self, game_id: i64) -> Result<GambleChoices> {
        self.container
            .call("gamble", game_id, &())
            .await
            .context("Failed to get gamble choice")
    }

    pub async fn get_fight_choice(
        &self,
        fight_info: &FightInfo,
        game_id: i64,
    ) -> Result<FightChoices> {
        self.container
            .call("fight", game_id, fight_info)
            .await
            .context("Failed to get fight choice")
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn player_state(&self) -> &PlayerState {
        &self.player_state
    }

    pub fn player_state_mut(&mut self) -> &mut PlayerState {
        &mut self.player_state
    }
}
