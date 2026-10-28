CREATE VIRTUAL TABLE IF NOT EXISTS search_index USING fts5(meeting_id UNINDEXED, start_seconds UNINDEXED, end_seconds UNINDEXED, title, tags, text, tokenize='unicode61 remove_diacritics 2');
