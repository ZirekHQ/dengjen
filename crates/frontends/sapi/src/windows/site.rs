//! [`Site`] over SAPI's `ISpTTSEngineSite`.

use windows::Win32::Foundation::{LPARAM, WPARAM};
use windows::Win32::Media::Speech::{
    ISpTTSEngineSite, SPEI_SENTENCE_BOUNDARY, SPEI_TTS_BOOKMARK, SPEI_WORD_BOUNDARY,
    SPET_LPARAM_IS_STRING, SPET_LPARAM_IS_UNDEFINED, SPEVENT, SPEVENTENUM, SPEVENTLPARAMTYPE,
    SPVES_ABORT, SPVES_SKIP, SPVSKIPTYPE, SPVST_SENTENCE,
};

use crate::speak::{Actions, Event, Site};

// Every event is handed to SAPI, which drops the ones the client did not ask for. Filtering here
// on `GetEventInterest` lost bookmarks in testing, and the events are cheap.
pub(super) struct EngineSite<'a> {
    site: &'a ISpTTSEngineSite,
}

impl<'a> EngineSite<'a> {
    pub(super) fn new(site: &'a ISpTTSEngineSite) -> Self {
        Self { site }
    }
}

fn header(id: SPEVENTENUM, lparam: SPEVENTLPARAMTYPE) -> i32 {
    (id.0 & 0xFFFF) | (lparam.0 << 16)
}

fn boundary(id: SPEVENTENUM, audio_offset: u64, offset: u32, len: u32) -> SPEVENT {
    SPEVENT {
        _bitfield: header(id, SPET_LPARAM_IS_UNDEFINED),
        ulStreamNum: 0,
        ullAudioStreamOffset: audio_offset,
        wParam: WPARAM(len as usize),
        lParam: LPARAM(offset as isize),
    }
}

impl Site for EngineSite<'_> {
    fn actions(&self) -> Actions {
        // SAFETY: a plain COM call on the site SAPI passed to `Speak`, which outlives it.
        let actions = unsafe { self.site.GetActions() };
        Actions {
            abort: actions & SPVES_ABORT.0 as u32 != 0,
            skip: actions & SPVES_SKIP.0 as u32 != 0,
        }
    }

    fn rate(&self) -> i32 {
        // SAFETY: a plain COM call on the site SAPI passed to `Speak`, which outlives it.
        unsafe { self.site.GetRate() }.unwrap_or(0)
    }

    fn volume(&self) -> u16 {
        // SAFETY: a plain COM call on the site SAPI passed to `Speak`, which outlives it.
        unsafe { self.site.GetVolume() }.unwrap_or(100)
    }

    fn skip_request(&self) -> i32 {
        let mut kind = SPVSKIPTYPE(0);
        let mut count = 0i32;
        // SAFETY: both out-parameters are live, writable locals for the call's duration.
        let asked = unsafe { self.site.GetSkipInfo(&mut kind, &mut count) };
        match asked {
            Ok(()) if kind == SPVST_SENTENCE => count,
            _ => 0,
        }
    }

    fn complete_skip(&self, skipped: i32) {
        // SAFETY: a plain COM call on the site SAPI passed to `Speak`, which outlives it.
        let _ = unsafe { self.site.CompleteSkip(skipped) };
    }

    fn write(&self, audio: &[u8]) -> Result<(), String> {
        let mut rest = audio;
        while !rest.is_empty() {
            let len = u32::try_from(rest.len()).unwrap_or(u32::MAX);
            // SAFETY: `rest` is a live byte slice of at least `len` bytes; SAPI copies it.
            let written = unsafe { self.site.Write(rest.as_ptr().cast(), len) }
                .map_err(|error| error.message())?;
            let taken = crate::speak::bytes_taken(len, written);
            rest = rest.get(taken as usize..).unwrap_or_default();
        }
        Ok(())
    }

    fn add_events(&self, events: &[Event]) -> Result<(), String> {
        // Bookmark names must stay alive until `AddEvents` has copied them.
        let mut names: Vec<Vec<u16>> = Vec::new();
        let mut raw: Vec<SPEVENT> = Vec::new();
        for event in events {
            match event {
                Event::Sentence {
                    audio_offset,
                    offset,
                    len,
                } => {
                    raw.push(boundary(
                        SPEI_SENTENCE_BOUNDARY,
                        *audio_offset,
                        *offset,
                        *len,
                    ));
                }
                Event::Word {
                    audio_offset,
                    offset,
                    len,
                } => {
                    raw.push(boundary(SPEI_WORD_BOUNDARY, *audio_offset, *offset, *len));
                }
                Event::Bookmark { audio_offset, name } => {
                    let wide: Vec<u16> = name.encode_utf16().chain(Some(0)).collect();
                    // SAPI documents wParam as the bookmark's leading number, as `_wtol` reads it.
                    let number = crate::speak::bookmark_number(name);
                    raw.push(SPEVENT {
                        _bitfield: header(SPEI_TTS_BOOKMARK, SPET_LPARAM_IS_STRING),
                        ulStreamNum: 0,
                        ullAudioStreamOffset: *audio_offset,
                        wParam: WPARAM(number as usize),
                        lParam: LPARAM(wide.as_ptr() as isize),
                    });
                    names.push(wide);
                }
            }
        }
        if raw.is_empty() {
            return Ok(());
        }
        let count = u32::try_from(raw.len()).map_err(|_| "too many events".to_owned())?;
        // SAFETY: `raw` holds `count` initialized events, and every string an event points to
        // is owned by `names`, which lives until after this call returns.
        let added = unsafe { self.site.AddEvents(raw.as_ptr(), count) };
        drop(names);
        added.map_err(|error| error.message())
    }
}
