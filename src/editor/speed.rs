use serde::{Deserialize, Serialize};

/// Сегмент с изменённой скоростью воспроизведения.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SpeedSegment {
    pub start_ms: u64,
    pub end_ms: u64,
    pub speed: f32,
}

/// Управление сегментами скорости.
pub struct SpeedManager {
    pub segments: Vec<SpeedSegment>,
}

impl SpeedManager {
    pub fn new() -> Self {
        Self {
            segments: Vec::new(),
        }
    }

    /// Добавить сегмент. Speed clamped к 0.25..4.0.
    pub fn add_segment(&mut self, start_ms: u64, end_ms: u64, speed: f32) {
        let speed = speed.clamp(0.25, 4.0);
        self.segments.push(SpeedSegment {
            start_ms,
            end_ms,
            speed,
        });
    }

    pub fn remove_segment(&mut self, index: usize) {
        if index < self.segments.len() {
            self.segments.remove(index);
        }
    }

    /// Скорость в данный момент. 1.0 если позиция не попадает ни в один сегмент.
    pub fn speed_at(&self, position_ms: u64) -> f32 {
        for seg in &self.segments {
            if position_ms >= seg.start_ms && position_ms < seg.end_ms {
                return seg.speed;
            }
        }
        1.0
    }

    /// Длительность воспроизведения с учётом скоростей.
    /// Сегмент 10с при speed=2.0 → 5с воспроизведения.
    pub fn effective_duration_ms(&self, original_duration_ms: u64) -> u64 {
        self.original_to_playback_ms(original_duration_ms)
    }

    /// Конвертация оригинального времени → время воспроизведения.
    /// Проходим от 0 до original_ms, суммируя реальное время с учётом скоростей.
    pub fn original_to_playback_ms(&self, original_ms: u64) -> u64 {
        let mut sorted = self.segments.clone();
        sorted.sort_by_key(|s| s.start_ms);

        let mut playback_ms: f64 = 0.0;
        let mut cursor: u64 = 0;

        for seg in &sorted {
            if cursor >= original_ms {
                break;
            }

            // Участок до сегмента — скорость 1.0
            let gap_end = seg.start_ms.min(original_ms);
            if cursor < gap_end {
                playback_ms += (gap_end - cursor) as f64;
                cursor = gap_end;
            }

            if cursor >= original_ms {
                break;
            }

            // Участок внутри сегмента
            let seg_end = seg.end_ms.min(original_ms);
            if cursor < seg_end {
                let chunk = (seg_end - cursor) as f64;
                playback_ms += chunk / seg.speed as f64;
                cursor = seg_end;
            }
        }

        // Остаток после всех сегментов — скорость 1.0
        if cursor < original_ms {
            playback_ms += (original_ms - cursor) as f64;
        }

        playback_ms.round() as u64
    }

    /// Конвертация времени воспроизведения → оригинальное время.
    pub fn playback_to_original_ms(&self, playback_ms: u64) -> u64 {
        let mut sorted = self.segments.clone();
        sorted.sort_by_key(|s| s.start_ms);

        let target = playback_ms as f64;
        let mut elapsed_playback: f64 = 0.0;
        let mut cursor: u64 = 0;

        for seg in &sorted {
            // Участок до сегмента — скорость 1.0
            let gap = seg.start_ms.saturating_sub(cursor) as f64;
            if elapsed_playback + gap >= target {
                return cursor + (target - elapsed_playback) as u64;
            }
            elapsed_playback += gap;
            cursor = seg.start_ms;

            // Участок внутри сегмента
            let seg_dur = (seg.end_ms - seg.start_ms) as f64;
            let playback_dur = seg_dur / seg.speed as f64;
            if elapsed_playback + playback_dur >= target {
                let remaining = target - elapsed_playback;
                let original_offset = remaining * seg.speed as f64;
                return cursor + original_offset as u64;
            }
            elapsed_playback += playback_dur;
            cursor = seg.end_ms;
        }

        // После всех сегментов — скорость 1.0
        cursor + (target - elapsed_playback) as u64
    }
}

impl Default for SpeedManager {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_speed_segment_creation() {
        let seg = SpeedSegment {
            start_ms: 1000,
            end_ms: 3000,
            speed: 2.0,
        };
        assert_eq!(seg.start_ms, 1000);
        assert_eq!(seg.end_ms, 3000);
        assert_eq!(seg.speed, 2.0);
    }

    #[test]
    fn test_speed_manager_empty() {
        let mgr = SpeedManager::new();
        assert_eq!(mgr.speed_at(0), 1.0);
        assert_eq!(mgr.speed_at(5000), 1.0);
        assert_eq!(mgr.effective_duration_ms(10000), 10000);
    }

    #[test]
    fn test_add_segment() {
        let mut mgr = SpeedManager::new();
        mgr.add_segment(1000, 3000, 2.0);
        assert_eq!(mgr.segments.len(), 1);
        assert_eq!(mgr.segments[0].speed, 2.0);

        // Clamp: too low
        mgr.add_segment(5000, 6000, 0.1);
        assert_eq!(mgr.segments[1].speed, 0.25);

        // Clamp: too high
        mgr.add_segment(7000, 8000, 10.0);
        assert_eq!(mgr.segments[2].speed, 4.0);
    }

    #[test]
    fn test_remove_segment() {
        let mut mgr = SpeedManager::new();
        mgr.add_segment(0, 1000, 2.0);
        mgr.add_segment(2000, 3000, 0.5);
        assert_eq!(mgr.segments.len(), 2);

        mgr.remove_segment(0);
        assert_eq!(mgr.segments.len(), 1);
        assert_eq!(mgr.segments[0].start_ms, 2000);

        // Out of bounds — no-op
        mgr.remove_segment(99);
        assert_eq!(mgr.segments.len(), 1);
    }

    #[test]
    fn test_speed_at() {
        let mut mgr = SpeedManager::new();
        mgr.add_segment(1000, 3000, 2.0);
        mgr.add_segment(5000, 7000, 0.5);

        assert_eq!(mgr.speed_at(0), 1.0);      // before any segment
        assert_eq!(mgr.speed_at(500), 1.0);
        assert_eq!(mgr.speed_at(1000), 2.0);    // start inclusive
        assert_eq!(mgr.speed_at(2000), 2.0);    // inside
        assert_eq!(mgr.speed_at(2999), 2.0);
        assert_eq!(mgr.speed_at(3000), 1.0);    // end exclusive
        assert_eq!(mgr.speed_at(5000), 0.5);
        assert_eq!(mgr.speed_at(6999), 0.5);
        assert_eq!(mgr.speed_at(7000), 1.0);
        assert_eq!(mgr.speed_at(99999), 1.0);
    }

    #[test]
    fn test_effective_duration_speedup() {
        // 10s video, segment 0-10s at 2x → 5s playback
        let mut mgr = SpeedManager::new();
        mgr.add_segment(0, 10000, 2.0);
        assert_eq!(mgr.effective_duration_ms(10000), 5000);
    }

    #[test]
    fn test_effective_duration_slowdown() {
        // 10s video, segment 0-10s at 0.5x → 20s playback
        let mut mgr = SpeedManager::new();
        mgr.add_segment(0, 10000, 0.5);
        assert_eq!(mgr.effective_duration_ms(10000), 20000);
    }

    #[test]
    fn test_effective_duration_mixed() {
        // 20s video:
        // 0-5s: 1.0x → 5s
        // 5-10s: 2.0x → 2.5s
        // 10-15s: 0.5x → 10s
        // 15-20s: 1.0x → 5s
        // Total: 22.5s → 22500ms
        let mut mgr = SpeedManager::new();
        mgr.add_segment(5000, 10000, 2.0);
        mgr.add_segment(10000, 15000, 0.5);
        assert_eq!(mgr.effective_duration_ms(20000), 22500);
    }

    #[test]
    fn test_original_to_playback() {
        let mut mgr = SpeedManager::new();
        // 0-5s normal, 5-10s at 2x, 10-20s normal
        mgr.add_segment(5000, 10000, 2.0);

        assert_eq!(mgr.original_to_playback_ms(0), 0);
        assert_eq!(mgr.original_to_playback_ms(5000), 5000);
        // 5s normal + 5s at 2x = 5s + 2.5s = 7.5s
        assert_eq!(mgr.original_to_playback_ms(10000), 7500);
        // + 5s normal = 12.5s
        assert_eq!(mgr.original_to_playback_ms(15000), 12500);
    }

    #[test]
    fn test_playback_to_original() {
        let mut mgr = SpeedManager::new();
        mgr.add_segment(5000, 10000, 2.0);

        assert_eq!(mgr.playback_to_original_ms(0), 0);
        assert_eq!(mgr.playback_to_original_ms(5000), 5000);
        // playback 7500 = 5s normal + 2.5s at 2x → original 10s
        assert_eq!(mgr.playback_to_original_ms(7500), 10000);
        // playback 12500 = 7.5s (covers 0-10s orig) + 5s normal → original 15s
        assert_eq!(mgr.playback_to_original_ms(12500), 15000);
    }

    #[test]
    fn test_roundtrip_conversion() {
        let mut mgr = SpeedManager::new();
        mgr.add_segment(2000, 5000, 2.0);
        mgr.add_segment(8000, 12000, 0.5);

        for orig_ms in [0, 1000, 2000, 3500, 5000, 7000, 8000, 10000, 12000, 15000] {
            let playback = mgr.original_to_playback_ms(orig_ms);
            let back = mgr.playback_to_original_ms(playback);
            assert!(
                (back as i64 - orig_ms as i64).unsigned_abs() <= 1,
                "roundtrip failed for {orig_ms}: playback={playback}, back={back}"
            );
        }
    }

    #[test]
    fn test_serialization() {
        let seg = SpeedSegment {
            start_ms: 1000,
            end_ms: 5000,
            speed: 1.5,
        };
        let json = serde_json::to_string(&seg).unwrap();
        let parsed: SpeedSegment = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, seg);
    }
}
