mod appkit;
mod files;
mod lanes;
mod process;
mod volumes;

pub use appkit::TextKitOne;
pub(crate) use appkit::constant;
#[cfg(test)]
pub use appkit::split_shortcut;
pub use appkit::{
    byte_range_to_ns, editing, from_nsrange, insert_text, item, layout_manager, now, ns, obj, perform_later, separator,
    shown_from_ns, shown_to_ns, storage, submenu, tagged, text_container, to_isize, to_nsrange, to_usize, without_undo,
};
pub use files::{file_name_of, read_regular_file, stamp};
#[cfg(feature = "selftest")]
pub use lanes::uncontained;
pub use lanes::{
    BACKGROUND, DISK, Lane, contained, free_in_background, guarded, main_after, main_soon, on_main, panic_is_contained,
    with_raised,
};
pub use process::{Process, end_child, process_exists};
pub use splat_file_state::Stamp;
pub use volumes::{BusyVolumes, answer_from, lane_of, lane_of_volume, volume_of};
