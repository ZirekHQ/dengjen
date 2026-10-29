use std::io::{self, Write};

const ESCAPE: u8 = 0x7d;
const INVERT: u8 = 1 << 5;

pub(crate) const BAD_SYNTAX: &str = "302 ERROR BAD SYNTAX";
pub(crate) const BAD_PARAM: &str = "303 ERROR INVALID PARAMETER OR VALUE";
pub(crate) const CANT_SPEAK: &str = "301 ERROR CANT SPEAK";
pub(crate) const MULTILINE: &str = "305 DATA MORE THAN ONE LINE";
pub(crate) const UNKNOWN: &str = "300 ERR UNKNOWN COMMAND";

pub(crate) fn line(output: &mut impl Write, text: &str) -> io::Result<()> {
    writeln!(output, "{text}")
}

fn escape_into(frame: &mut Vec<u8>, byte: u8) {
    if byte == b'\n' || byte == ESCAPE {
        frame.extend([ESCAPE, byte ^ INVERT]);
    } else {
        frame.push(byte);
    }
}

pub(crate) fn audio_frame(samples: &[i16], sample_rate: u32) -> Vec<u8> {
    let mut frame = format!(
        "705-bits=16\n705-num_channels=1\n705-sample_rate={sample_rate}\n705-num_samples={}\n705-big_endian=0\n705-AUDIO\0",
        samples.len()
    )
    .into_bytes();
    samples
        .iter()
        .flat_map(|sample| sample.to_le_bytes())
        .for_each(|byte| escape_into(&mut frame, byte));
    frame.extend(b"\n705 AUDIO\n");
    frame
}

#[cfg(test)]
mod tests {
    use super::*;

    fn payload(frame: &[u8]) -> &[u8] {
        let start = frame.iter().position(|b| *b == 0).unwrap() + 1;
        let end = frame.len() - b"\n705 AUDIO\n".len();
        &frame[start..end]
    }

    #[test]
    fn the_header_names_the_format() {
        let frame = audio_frame(&[1, 2, 3], 22050);
        let header = String::from_utf8_lossy(&frame[..frame.iter().position(|b| *b == 0).unwrap()]);
        assert_eq!(
            header,
            "705-bits=16\n705-num_channels=1\n705-sample_rate=22050\n705-num_samples=3\n705-big_endian=0\n705-AUDIO"
        );
        assert!(frame.ends_with(b"\n705 AUDIO\n"));
    }

    #[test]
    fn samples_are_little_endian_pcm() {
        assert_eq!(payload(&audio_frame(&[0x0201, -2], 1)), [1, 2, 0xfe, 0xff]);
    }

    #[test]
    fn newline_and_escape_bytes_are_escaped() {
        let frame = audio_frame(&[0x7d0a], 1);
        assert_eq!(payload(&frame), [0x7d, 0x2a, 0x7d, 0x5d]);
        assert!(!payload(&frame).contains(&b'\n'));
    }
}
