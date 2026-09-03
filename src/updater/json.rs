use super::GitHubAsset;
use v_concat::v_concat;

pub(super) fn json_string_field(object: &str, field: &str) -> Option<String> {
    let needle = v_concat!("\"{field}\"");
    let start = object.find(&needle)? + needle.len();
    let after_key = object[start..].trim_start();
    let after_colon = after_key.strip_prefix(':')?.trim_start();

    parse_json_string(after_colon).map(|(value, _)| value)
}

pub(super) fn parse_assets(json: &str) -> Vec<GitHubAsset> {
    let Some(assets_start) = json.find("\"assets\"") else {
        return Vec::new();
    };
    let Some(array_start) = json[assets_start..].find('[') else {
        return Vec::new();
    };
    let array_start = assets_start + array_start;
    let Some(array_end) = matching_json_container(json, array_start, '[', ']') else {
        return Vec::new();
    };
    let assets_json = &json[array_start + 1..array_end];
    let mut assets = Vec::new();
    let mut cursor = 0;

    while let Some(relative_start) = assets_json[cursor..].find('{') {
        let object_start = cursor + relative_start;
        let Some(object_end) = matching_json_container(assets_json, object_start, '{', '}') else {
            break;
        };
        let object = &assets_json[object_start..=object_end];
        if let (Some(name), Some(download_url)) = (
            json_string_field(object, "name"),
            json_string_field(object, "browser_download_url"),
        ) {
            assets.push(GitHubAsset { name, download_url });
        }
        cursor = object_end + 1;
    }

    assets
}

fn matching_json_container(value: &str, start: usize, open: char, close: char) -> Option<usize> {
    let mut in_string = false;
    let mut escaped = false;
    let mut depth = 0usize;

    for (index, ch) in value[start..].char_indices() {
        if in_string {
            if escaped {
                escaped = false;
            } else if ch == '\\' {
                escaped = true;
            } else if ch == '"' {
                in_string = false;
            }
            continue;
        }

        if ch == '"' {
            in_string = true;
        } else if ch == open {
            depth += 1;
        } else if ch == close {
            depth = depth.checked_sub(1)?;
            if depth == 0 {
                return Some(start + index);
            }
        }
    }

    None
}

fn parse_json_string(value: &str) -> Option<(String, usize)> {
    let mut chars = value.char_indices();
    let (_, first) = chars.next()?;
    if first != '"' {
        return None;
    }

    let mut out = String::new();
    let mut escaped = false;

    for (index, ch) in chars {
        if escaped {
            match ch {
                '"' => out.push('"'),
                '\\' => out.push('\\'),
                '/' => out.push('/'),
                'b' => out.push('\u{08}'),
                'f' => out.push('\u{0c}'),
                'n' => out.push('\n'),
                'r' => out.push('\r'),
                't' => out.push('\t'),
                'u' => out.push('?'),
                other => out.push(other),
            }
            escaped = false;
            continue;
        }

        if ch == '\\' {
            escaped = true;
            continue;
        }

        if ch == '"' {
            return Some((out, index + ch.len_utf8()));
        }

        out.push(ch);
    }

    None
}

#[cfg(test)]
mod tests {
    use super::parse_assets;

    #[test]
    fn parses_release_assets_from_github_json() {
        let assets = parse_assets(
            r#"{
                "tag_name": "v1.2.3",
                "assets": [
                    {
                        "name": "v_fs_backup_v1.2.3_linux_x86_64",
                        "browser_download_url": "https://example.test/v_fs_backup"
                    }
                ]
            }"#,
        );

        assert_eq!(assets.len(), 1);
        assert_eq!(assets[0].name, "v_fs_backup_v1.2.3_linux_x86_64");
        assert_eq!(assets[0].download_url, "https://example.test/v_fs_backup");
    }
}
