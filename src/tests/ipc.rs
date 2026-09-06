use crate::ipc::{encode_frame, socket_paths};

#[test]
fn frame_header_is_op_then_le_len() {
    let frame = encode_frame(1, "abc");
    assert_eq!(&frame[..4], &1u32.to_le_bytes());
    assert_eq!(&frame[4..8], &3u32.to_le_bytes());
    assert_eq!(&frame[8..], b"abc");
}

#[test]
fn socket_paths_cover_runtime_dir_and_tmp() {
    let paths = socket_paths();
    assert!(paths.iter().any(|p| p.ends_with("discord-ipc-0")));
    assert!(paths.iter().any(|p| p.ends_with("discord-ipc-9")));
}
