use std::fs::File;
use std::path::Path;
use std::time::Duration;

use rodio::{Decoder, DeviceSinkBuilder, MixerDeviceSink, Player};

pub struct Audio {
    _device: MixerDeviceSink,
    player: Player,
}

impl Audio {
    pub fn new() -> Result<Self, String> {
        let device = DeviceSinkBuilder::open_default_sink().map_err(|error| error.to_string())?;
        let player = Player::connect_new(device.mixer());
        Ok(Self { _device: device, player })
    }

    pub fn load_and_play(&self, path: &Path) -> Result<(), String> {
        let file = File::open(path).map_err(|error| error.to_string())?;
        let source = Decoder::try_from(file).map_err(|error| error.to_string())?;
        self.player.clear();
        self.player.append(source);
        self.player.play();
        Ok(())
    }

    pub fn pause_or_resume(&self) {
        if self.player.is_paused() {
            self.player.play();
        } else {
            self.player.pause();
        }
    }

    pub fn stop(&self) {
        self.player.stop();
    }

    pub fn is_paused(&self) -> bool {
        self.player.is_paused()
    }

    pub fn is_empty(&self) -> bool {
        self.player.empty()
    }

    pub fn set_volume(&self, volume: f32) {
        self.player.set_volume(volume);
    }

    pub fn position(&self) -> Duration {
        self.player.get_pos()
    }

    pub fn seek(&self, position: Duration) -> Result<(), String> {
        self.player.try_seek(position).map_err(|error| error.to_string())
    }
}
