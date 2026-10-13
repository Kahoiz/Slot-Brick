#![cfg(feature = "brick")]
use slot::app::App;
use slot_input::Action;
#[test]
fn switch_mutes_and_restores_selected_volume_without_changing_it() {
    let mut app=App::new(Vec::new());app.apply(Action::VolumeUp);
    let before=app.output_volume();assert!(before>0);
    app.set_brick_mute(false);app.set_brick_mute(true);
    assert!(app.muted());assert_eq!(app.output_volume(),0);
    app.set_brick_mute(true);assert_eq!(app.output_volume(),0);
    app.set_brick_mute(false);assert_eq!(app.output_volume(),before);assert!(!app.muted());
}
#[test]
fn volume_keys_cannot_override_engaged_mute_switch() {
    let mut app=App::new(Vec::new());app.set_brick_mute(true);
    app.apply(Action::VolumeUp);assert_eq!(app.output_volume(),0);
    app.set_brick_mute(false);assert!(app.output_volume()>0);
}
