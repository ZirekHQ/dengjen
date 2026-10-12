use dengjen_tts_core::{DengjenError, DengjenResult};

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Word {
    pub text: String,
    pub start: usize,
    pub end: usize,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Token {
    pub text: String,
    pub start: usize,
    pub end: usize,
}

pub(crate) fn wordize(text: &str) -> (Vec<Word>, Vec<Option<usize>>) {
    let chars: Vec<char> = text.chars().collect();
    let mut words: Vec<Word> = Vec::new();
    let mut text_to_word: Vec<Option<usize>> = Vec::with_capacity(chars.len());

    let mut i = 0;
    while let Some(&current) = chars.get(i) {
        if current == ' ' {
            text_to_word.push(None);
            i += 1;
            continue;
        }
        if current.is_ascii_alphanumeric() {
            let start = i;
            while chars.get(i).is_some_and(char::is_ascii_alphanumeric) {
                i += 1;
            }
            let word_index = words.len();
            words.push(Word {
                text: chars.iter().skip(start).take(i - start).collect(),
                start,
                end: i,
            });
            for _ in start..i {
                text_to_word.push(Some(word_index));
            }
            continue;
        }
        let word_index = words.len();
        words.push(Word {
            text: current.to_string(),
            start: i,
            end: i + 1,
        });
        text_to_word.push(Some(word_index));
        i += 1;
    }

    (words, text_to_word)
}

pub(crate) fn tokenize_and_map(
    tokenizer: &tokenizers::Tokenizer,
    text: &str,
) -> DengjenResult<(Vec<Token>, Vec<Option<usize>>)> {
    let (words, text_to_word) = wordize(text);
    let chars: Vec<char> = text.chars().collect();

    let mut tokens: Vec<Token> = Vec::new();
    let mut text_to_token: Vec<Option<usize>> = vec![None; chars.len()];

    for word in &words {
        let encoding = tokenizer
            .encode(word.text.as_str(), false)
            .map_err(|cause| {
                DengjenError::PhonemizationError(format!(
                    "g2pW tokenizer failed to encode {:?}: {cause}",
                    word.text
                ))
            })?;
        let word_tokens: Vec<String> = encoding
            .get_tokens()
            .iter()
            .map(|t| t.to_string())
            .collect();

        if word_tokens.is_empty() || word_tokens == ["[UNK]"] {
            let token_index = tokens.len();
            tokens.push(Token {
                text: "[UNK]".to_string(),
                start: word.start,
                end: word.end,
            });
            text_to_token
                .iter_mut()
                .skip(word.start)
                .take(word.end.saturating_sub(word.start))
                .for_each(|slot| *slot = Some(token_index));
            continue;
        }

        let mut cursor = word.start;
        for word_token in word_tokens {
            let stripped = word_token.strip_prefix("##").unwrap_or(&word_token);
            let token_len = stripped.chars().count();
            let token_index = tokens.len();
            tokens.push(Token {
                text: word_token.clone(),
                start: cursor,
                end: cursor + token_len,
            });
            for pos in cursor..cursor + token_len {
                if let Some(slot) = text_to_token.get_mut(pos) {
                    *slot = Some(token_index);
                }
            }
            cursor += token_len;
        }
    }

    let _ = text_to_word;
    Ok((tokens, text_to_token))
}

#[cfg(test)]
mod tests {
    #[test]
    fn wordize_groups_runs_of_ascii_alphanumerics_into_one_word() {
        let (words, text_to_word) = super::wordize("ab12 你");
        assert_eq!(words.len(), 2);
        assert_eq!(words[0].text, "ab12");
        assert_eq!(words[0].start, 0);
        assert_eq!(words[0].end, 4);
        assert_eq!(words[1].text, "你");
        assert_eq!(words[1].start, 5);
        assert_eq!(words[1].end, 6);
        assert_eq!(
            text_to_word,
            vec![Some(0), Some(0), Some(0), Some(0), None, Some(1)]
        );
    }

    #[test]
    fn wordize_treats_every_non_ascii_alnum_character_as_its_own_word() {
        let (words, _text_to_word) = super::wordize("你好");
        assert_eq!(words.len(), 2);
        assert_eq!(words[0].text, "你");
        assert_eq!(words[1].text, "好");
    }

    #[test]
    fn wordize_maps_spaces_to_none_without_producing_a_word() {
        let (words, text_to_word) = super::wordize("  你");
        assert_eq!(words.len(), 1);
        assert_eq!(text_to_word, vec![None, None, Some(0)]);
    }

    #[test]
    fn tokenize_and_map_produces_one_token_per_single_char_word_when_not_split_further() {
        let Some(tokenizer) = test_tokenizer() else {
            eprintln!("skipping: DENGJEN_PINYIN_TEST_TOKENIZER_PATH not set");
            return;
        };
        let (tokens, text_to_token) = super::tokenize_and_map(&tokenizer, "你好").unwrap();
        assert_eq!(tokens.len(), 2);
        assert_eq!(tokens[0].start, 0);
        assert_eq!(tokens[0].end, 1);
        assert_eq!(tokens[1].start, 1);
        assert_eq!(tokens[1].end, 2);
        assert_eq!(text_to_token, vec![Some(0), Some(1)]);
    }

    #[test]
    fn tokenize_and_map_leaves_whitespace_positions_unmapped() {
        let Some(tokenizer) = test_tokenizer() else {
            eprintln!("skipping: DENGJEN_PINYIN_TEST_TOKENIZER_PATH not set");
            return;
        };
        let (_tokens, text_to_token) = super::tokenize_and_map(&tokenizer, " 你").unwrap();

        assert_eq!(text_to_token[0], None);
        assert_eq!(text_to_token[1], Some(0));
    }

    fn test_tokenizer() -> Option<tokenizers::Tokenizer> {
        let path = std::env::var("DENGJEN_PINYIN_TEST_TOKENIZER_PATH").ok()?;
        Some(tokenizers::Tokenizer::from_file(path).unwrap())
    }

    /// A tiny, self-contained WordPiece tokenizer for exercising `tokenize_and_map` without a
    /// downloaded model: real `tokenizers::Tokenizer` behavior, just a hand-sized vocab.
    fn tiny_wordpiece_tokenizer() -> tokenizers::Tokenizer {
        let json = r###"{
            "version": "1.0",
            "truncation": null,
            "padding": null,
            "added_tokens": [],
            "normalizer": null,
            "pre_tokenizer": null,
            "post_processor": null,
            "decoder": null,
            "model": {
                "type": "WordPiece",
                "unk_token": "[UNK]",
                "continuing_subword_prefix": "##",
                "max_input_chars_per_word": 100,
                "vocab": {
                    "[UNK]": 0,
                    "你": 1,
                    "好": 2,
                    "hell": 3,
                    "##o": 4
                }
            }
        }"###;
        tokenizers::Tokenizer::from_bytes(json.as_bytes()).unwrap()
    }

    #[test]
    fn tokenize_and_map_produces_one_token_per_vocabulary_word() {
        let tokenizer = tiny_wordpiece_tokenizer();
        let (tokens, text_to_token) = super::tokenize_and_map(&tokenizer, "你好").unwrap();
        assert_eq!(tokens.len(), 2);
        assert_eq!(tokens[0].text, "你");
        assert_eq!(tokens[1].text, "好");
        assert_eq!(text_to_token, vec![Some(0), Some(1)]);
    }

    #[test]
    fn tokenize_and_map_splits_a_word_into_a_root_and_a_continuation_subword() {
        let tokenizer = tiny_wordpiece_tokenizer();
        let (tokens, text_to_token) = super::tokenize_and_map(&tokenizer, "hello").unwrap();
        assert_eq!(
            tokens.iter().map(|t| t.text.as_str()).collect::<Vec<_>>(),
            vec!["hell", "##o"]
        );
        assert_eq!(tokens[0].start, 0);
        assert_eq!(tokens[0].end, 4);
        assert_eq!(tokens[1].start, 4);
        assert_eq!(tokens[1].end, 5);
        assert_eq!(
            text_to_token,
            vec![Some(0), Some(0), Some(0), Some(0), Some(1)]
        );
    }

    #[test]
    fn tokenize_and_map_collapses_an_out_of_vocabulary_word_to_a_single_unk_token() {
        let tokenizer = tiny_wordpiece_tokenizer();
        let (tokens, text_to_token) = super::tokenize_and_map(&tokenizer, "汉").unwrap();
        assert_eq!(tokens.len(), 1);
        assert_eq!(tokens[0].text, "[UNK]");
        assert_eq!(tokens[0].start, 0);
        assert_eq!(tokens[0].end, 1);
        assert_eq!(text_to_token, vec![Some(0)]);
    }

    #[test]
    fn tokenize_and_map_leaves_whitespace_positions_unmapped_with_a_tiny_tokenizer() {
        let tokenizer = tiny_wordpiece_tokenizer();
        let (_tokens, text_to_token) = super::tokenize_and_map(&tokenizer, " 你").unwrap();
        assert_eq!(text_to_token[0], None);
        assert_eq!(text_to_token[1], Some(0));
    }
}
