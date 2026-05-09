use ratatui::{layout::Margin, style::Color};
use tachyonfx::{fx, fx::RepeatMode, CellFilter, Effect, Interpolation, Motion};

use crate::domain::AnimationStage;

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
        AnimationStage::RepoMaterialize => fx::parallel(&[
            fx::coalesce((650, Interpolation::CircOut)),
            fx::slide_in(
                Motion::UpToDown,
                10,
                1,
                Color::Black,
                (520, Interpolation::QuadOut),
            ),
        ])
        .with_filter(text_only),
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
        AnimationStage::ImpactToFutures => fx::sequence(&[
            fx::slide_out(
                Motion::RightToLeft,
                6,
                1,
                Color::Black,
                (240, Interpolation::QuadIn),
            ),
            fx::parallel(&[
                fx::slide_in(
                    Motion::RightToLeft,
                    10,
                    1,
                    Color::Black,
                    (360, Interpolation::CircOut),
                ),
                fx::sweep_in(
                    Motion::LeftToRight,
                    10,
                    1,
                    Color::Cyan,
                    (420, Interpolation::CircOut),
                ),
            ]),
        ])
        .with_filter(text_only),
        AnimationStage::FuturesToImpact => fx::sequence(&[
            fx::slide_out(
                Motion::LeftToRight,
                6,
                1,
                Color::Black,
                (240, Interpolation::QuadIn),
            ),
            fx::parallel(&[
                fx::slide_in(
                    Motion::LeftToRight,
                    10,
                    1,
                    Color::Black,
                    (360, Interpolation::CircOut),
                ),
                fx::fade_to_fg(Color::Green, (320, Interpolation::QuadOut)),
            ]),
        ])
        .with_filter(text_only),
        AnimationStage::DiagramReveal => fx::sequence(&[
            fx::parallel(&[
                fx::dissolve((260, Interpolation::Linear)),
                fx::coalesce((520, Interpolation::CircOut)),
            ]),
            fx::sweep_in(
                Motion::LeftToRight,
                18,
                1,
                Color::Cyan,
                (520, Interpolation::CircOut),
            ),
        ])
        .with_filter(text_only),
    }
}
