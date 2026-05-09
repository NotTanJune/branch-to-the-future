use ratatui::{
    layout::Margin,
    style::{Color, Style},
};
use tachyonfx::{
    fx::{self, ExpandDirection, RepeatMode},
    CellFilter, Effect, EffectTimer, Interpolation, Motion,
};

use crate::domain::AnimationStage;

#[cfg(test)]
const SCREEN_TRANSITION_STAGES: [AnimationStage; 4] = [
    AnimationStage::RepoMaterialize,
    AnimationStage::ImpactToFutures,
    AnimationStage::FuturesToImpact,
    AnimationStage::DiagramReveal,
];
const SCREEN_TRANSITION_MS: u32 = 1600;

pub fn stage_effect(stage: AnimationStage) -> Effect {
    let text_only = CellFilter::AllOf(vec![CellFilter::Inner(Margin::new(1, 1)), CellFilter::Text]);
    match stage {
        AnimationStage::BootReveal => fx::sequence(&[
            fx::parallel(&[
                fx::coalesce((700, Interpolation::ExpoOut)),
                fx::fade_from_fg(Color::DarkGray, (600, Interpolation::QuadOut)),
            ]),
            fx::sweep_in(
                Motion::LeftToRight,
                18,
                2,
                Color::Blue,
                (520, Interpolation::CircOut),
            ),
        ])
        .with_filter(text_only),
        AnimationStage::RepoMaterialize => screen_bounce_transition(),
        AnimationStage::ScanningSweep => fx::parallel(&[
            fx::sweep_in(
                Motion::LeftToRight,
                18,
                1,
                Color::Cyan,
                (620, Interpolation::CircOut),
            ),
            fx::fade_from_fg(Color::DarkGray, (420, Interpolation::QuadOut)),
        ])
        .with_filter(text_only),
        AnimationStage::StreamShimmer => fx::repeat(
            fx::ping_pong(
                fx::hsl_shift_fg([205.0, 28.0, 10.0], (380, Interpolation::SineInOut))
                    .with_filter(text_only),
            ),
            RepeatMode::Times(2),
        ),
        AnimationStage::ImpactTrace => fx::parallel(&[
            fx::slide_in(
                Motion::LeftToRight,
                6,
                0,
                Color::Black,
                (260, Interpolation::QuadOut),
            ),
            fx::sweep_in(
                Motion::LeftToRight,
                8,
                0,
                Color::Yellow,
                (320, Interpolation::CircOut),
            ),
        ])
        .with_filter(text_only),
        AnimationStage::RiskBloom => fx::repeat(
            fx::ping_pong(
                fx::hsl_shift_fg([0.0, 65.0, 28.0], (420, Interpolation::SineInOut))
                    .with_filter(text_only),
            ),
            RepeatMode::Times(3),
        ),
        AnimationStage::LockIn => fx::parallel(&[
            fx::coalesce((320, Interpolation::Linear)),
            fx::fade_to_fg(Color::Green, (260, Interpolation::QuadOut)),
        ])
        .with_filter(text_only),
        AnimationStage::ReplayTrace => fx::sequence(&[
            fx::dissolve((220, Interpolation::Linear)),
            fx::sweep_in(
                Motion::LeftToRight,
                28,
                3,
                Color::Yellow,
                (760, Interpolation::CircOut),
            ),
            fx::coalesce((300, Interpolation::QuadOut)),
        ])
        .with_filter(text_only),
        AnimationStage::ImpactToFutures
        | AnimationStage::FuturesToImpact
        | AnimationStage::DiagramReveal => screen_bounce_transition(),
    }
}

fn screen_bounce_transition() -> Effect {
    let style = Style::default()
        .fg(Color::from_u32(0x32302F))
        .bg(Color::from_u32(0x1D2021));

    fx::expand(
        ExpandDirection::Horizontal,
        style,
        EffectTimer::from_ms(SCREEN_TRANSITION_MS, Interpolation::BounceOut),
    )
}

#[cfg(test)]
mod tests {
    use ratatui::{buffer::Buffer, layout::Rect};
    use tachyonfx::Duration;

    use super::*;

    #[test]
    fn screen_transition_stages_use_terminal_area() {
        for stage in SCREEN_TRANSITION_STAGES {
            assert_eq!(stage_effect(stage).area(), None, "{stage:?}");
        }
    }

    #[test]
    fn screen_transition_keeps_running_for_visible_bounce() {
        let area = Rect::new(0, 0, 120, 40);
        let mut buffer = Buffer::empty(area);
        let mut effect = stage_effect(AnimationStage::ImpactToFutures);

        effect.process(Duration::from_millis(1000), &mut buffer, area);

        assert!(
            effect.running(),
            "screen transition should still be running at 1000ms so BounceOut is visible"
        );

        effect.process(Duration::from_millis(600), &mut buffer, area);

        assert!(
            !effect.running(),
            "screen transition should finish after the configured 1600ms bounce"
        );
    }
}
