mod appkit;
mod files;
mod lanes;
mod process;
mod volumes;
#[allow(dead_code, reason = "the app connects and the service serves; each uses its half")]
pub mod xpc;

pub use appkit::TextKitOne;
pub(crate) use appkit::constant;
#[cfg(test)]
pub use appkit::split_shortcut;
pub use appkit::{
    button, byte_range_to_ns, editing, error_handler, from_nsrange, image_button, insert_text, item, layout_manager,
    now, ns, obj, open_handler, perform_later, separator, set_action, set_represented, set_target, set_toolbar_action,
    shown_from_ns, shown_to_ns, storage, submenu, tagged, text_container, to_isize, to_nsrange, to_usize, without_undo,
};
pub use files::{file_name_of, read_regular_file, real_home, stamp};
#[cfg(feature = "selftest")]
pub use lanes::uncontained;
pub use lanes::{
    BACKGROUND, DISK, Lane, contained, free_in_background, guarded, main_after, main_soon, on_main, panic_is_contained,
    with_raised,
};
pub use process::process_exists;
pub use splat_file_state::Stamp;
pub use volumes::{BusyVolumes, answer_from, lane_of, lane_of_volume, volume_of};
