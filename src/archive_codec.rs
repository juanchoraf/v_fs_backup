fn manifest_to_value(manifest: &ArchiveManifest) -> Vec<u8> {
    let mut payload = Vec::new();
    write_u32(&mut payload, manifest.format_version);
    write_i64(&mut payload, manifest.created_unix_seconds);
    write_string(&mut payload, &manifest.source_os);
    payload
}

fn manifest_from_slice(payload: &[u8]) -> Result<ArchiveManifest> {
    let mut reader = PayloadReader::new(payload);
    let manifest = ArchiveManifest {
        format_version: reader.read_u32()?,
        created_unix_seconds: reader.read_i64()?,
        source_os: reader.read_string()?,
    };
    reader.finish()?;
    Ok(manifest)
}

fn require_archive_source_os(source_os: &Option<String>) -> Result<&str> {
    source_os
        .as_deref()
        .context("archive entry appeared before archive manifest")
}

fn metadata_platform_for_source_os(source_os: &str) -> MetadataPlatform {
    if source_os.eq_ignore_ascii_case("windows") {
        MetadataPlatform::Windows
    } else {
        MetadataPlatform::Unix
    }
}

fn entry_metadata_to_value(meta: &EntryMetadata) -> Vec<u8> {
    let mut payload = Vec::new();
    write_string(&mut payload, &meta.path);
    write_u8(&mut payload, entry_kind_to_byte(meta.kind));
    write_u64(&mut payload, meta.len);
    write_bool(&mut payload, meta.readonly);
    write_file_stamp_option(&mut payload, meta.modified);
    write_file_stamp_option(&mut payload, meta.accessed);
    write_file_stamp_option(&mut payload, meta.created);
    #[cfg(unix)]
    write_unix_metadata_option(&mut payload, meta.unix);
    #[cfg(windows)]
    write_windows_metadata_option(&mut payload, meta.windows);
    payload
}

fn entry_metadata_from_slice_for_os(payload: &[u8], source_os: &str) -> Result<EntryMetadata> {
    let mut reader = PayloadReader::new(payload);
    let meta = entry_metadata_from_reader_for_os(&mut reader, source_os)?;
    reader.finish()?;
    Ok(meta)
}

fn entry_metadata_from_reader_for_os(
    reader: &mut PayloadReader<'_>,
    source_os: &str,
) -> Result<EntryMetadata> {
    let path = reader.read_string()?;
    let kind = entry_kind_from_byte(reader.read_u8()?)?;
    let len = reader.read_u64()?;
    let readonly = reader.read_bool()?;
    let modified = reader.read_file_stamp_option()?;
    let accessed = reader.read_file_stamp_option()?;
    let created = reader.read_file_stamp_option()?;
    let metadata_platform = metadata_platform_for_source_os(source_os);
    #[cfg(unix)]
    let unix = match metadata_platform {
        MetadataPlatform::Unix => {
            reader
                .read_unix_metadata_option_fields()?
                .map(|(mode, uid, gid, rdev)| UnixMetadata {
                    mode,
                    uid,
                    gid,
                    rdev,
                })
        }
        MetadataPlatform::Windows => {
            let _ = reader.read_windows_metadata_option_fields()?;
            None
        }
    };
    #[cfg(windows)]
    let windows = match metadata_platform {
        MetadataPlatform::Windows => reader
            .read_windows_metadata_option_fields()?
            .map(|file_attributes| WindowsMetadata { file_attributes }),
        MetadataPlatform::Unix => {
            let _ = reader.read_unix_metadata_option_fields()?;
            None
        }
    };

    Ok(EntryMetadata {
        path,
        kind,
        len,
        readonly,
        modified,
        accessed,
        created,
        #[cfg(unix)]
        unix,
        #[cfg(windows)]
        windows,
    })
}

fn symlink_record_to_value(record: &SymlinkRecord) -> Vec<u8> {
    let mut payload = entry_metadata_to_value(&record.meta);
    write_string(&mut payload, &record.target);
    write_bool_option(&mut payload, record.target_is_dir);
    payload
}

fn symlink_record_from_slice_for_os(payload: &[u8], source_os: &str) -> Result<SymlinkRecord> {
    let mut reader = PayloadReader::new(payload);
    let record = SymlinkRecord {
        meta: entry_metadata_from_reader_for_os(&mut reader, source_os)?,
        target: reader.read_string()?,
        target_is_dir: reader.read_bool_option()?,
    };
    reader.finish()?;
    Ok(record)
}

fn file_data_record_to_value(record: &FileDataRecord) -> Vec<u8> {
    let mut payload = entry_metadata_to_value(&record.meta);
    write_string(&mut payload, &record.hash);
    write_u64(&mut payload, record.data_len);
    payload
}

fn file_data_record_from_slice_for_os(payload: &[u8], source_os: &str) -> Result<FileDataRecord> {
    let mut reader = PayloadReader::new(payload);
    let record = FileDataRecord {
        meta: entry_metadata_from_reader_for_os(&mut reader, source_os)?,
        hash: reader.read_string()?,
        data_len: reader.read_u64()?,
    };
    reader.finish()?;
    Ok(record)
}

fn file_ref_record_to_value(record: &FileRefRecord) -> Vec<u8> {
    let mut payload = entry_metadata_to_value(&record.meta);
    write_string(&mut payload, &record.hash);
    write_string(&mut payload, &record.original_path);
    payload
}

fn file_ref_record_from_slice_for_os(payload: &[u8], source_os: &str) -> Result<FileRefRecord> {
    let mut reader = PayloadReader::new(payload);
    let record = FileRefRecord {
        meta: entry_metadata_from_reader_for_os(&mut reader, source_os)?,
        hash: reader.read_string()?,
        original_path: reader.read_string()?,
    };
    reader.finish()?;
    Ok(record)
}

fn entry_kind_to_byte(kind: EntryKind) -> u8 {
    match kind {
        EntryKind::File => 1,
        EntryKind::Directory => 2,
        EntryKind::Symlink => 3,
    }
}

fn entry_kind_from_byte(value: u8) -> Result<EntryKind> {
    match value {
        1 => Ok(EntryKind::File),
        2 => Ok(EntryKind::Directory),
        3 => Ok(EntryKind::Symlink),
        _ => bail!("invalid archive entry kind byte {value}"),
    }
}

fn write_u8(payload: &mut Vec<u8>, value: u8) {
    payload.push(value);
}

fn write_bool(payload: &mut Vec<u8>, value: bool) {
    write_u8(payload, u8::from(value));
}

fn write_bool_option(payload: &mut Vec<u8>, value: Option<bool>) {
    match value {
        Some(value) => {
            write_u8(payload, 1);
            write_bool(payload, value);
        }
        None => write_u8(payload, 0),
    }
}

fn write_u32(payload: &mut Vec<u8>, value: u32) {
    payload.extend_from_slice(&value.to_le_bytes());
}

fn write_u64(payload: &mut Vec<u8>, value: u64) {
    payload.extend_from_slice(&value.to_le_bytes());
}

fn write_i64(payload: &mut Vec<u8>, value: i64) {
    payload.extend_from_slice(&value.to_le_bytes());
}

fn write_string(payload: &mut Vec<u8>, value: &str) {
    write_u64(payload, value.len() as u64);
    payload.extend_from_slice(value.as_bytes());
}

fn write_file_stamp_option(payload: &mut Vec<u8>, stamp: Option<FileStamp>) {
    match stamp {
        Some(stamp) => {
            write_u8(payload, 1);
            write_i64(payload, stamp.seconds);
            write_u32(payload, stamp.nanos);
        }
        None => write_u8(payload, 0),
    }
}

#[cfg(unix)]
fn write_unix_metadata_option(payload: &mut Vec<u8>, meta: Option<UnixMetadata>) {
    match meta {
        Some(meta) => {
            write_u8(payload, 1);
            write_u32(payload, meta.mode);
            write_u32(payload, meta.uid);
            write_u32(payload, meta.gid);
            write_u64(payload, meta.rdev);
        }
        None => write_u8(payload, 0),
    }
}

#[cfg(windows)]
fn write_windows_metadata_option(payload: &mut Vec<u8>, meta: Option<WindowsMetadata>) {
    match meta {
        Some(meta) => {
            write_u8(payload, 1);
            write_u32(payload, meta.file_attributes);
        }
        None => write_u8(payload, 0),
    }
}

struct PayloadReader<'a> {
    payload: &'a [u8],
    offset: usize,
}

impl<'a> PayloadReader<'a> {
    fn new(payload: &'a [u8]) -> Self {
        Self { payload, offset: 0 }
    }

    fn finish(&self) -> Result<()> {
        if self.offset == self.payload.len() {
            Ok(())
        } else {
            bail!(
                "archive record has {} trailing byte(s)",
                self.payload.len() - self.offset
            )
        }
    }

    fn read_exact(&mut self, len: usize) -> Result<&'a [u8]> {
        let end = self
            .offset
            .checked_add(len)
            .context("archive record length overflow")?;
        if end > self.payload.len() {
            bail!(
                "archive record ended unexpectedly: needed {} byte(s) at offset {}, payload has {} byte(s)",
                len,
                self.offset,
                self.payload.len()
            );
        }
        let bytes = &self.payload[self.offset..end];
        self.offset = end;
        Ok(bytes)
    }

    fn read_u8(&mut self) -> Result<u8> {
        Ok(self.read_exact(1)?[0])
    }

    fn read_bool(&mut self) -> Result<bool> {
        match self.read_u8()? {
            0 => Ok(false),
            1 => Ok(true),
            value => bail!("invalid bool byte {value} in archive record"),
        }
    }

    fn read_bool_option(&mut self) -> Result<Option<bool>> {
        match self.read_u8()? {
            0 => Ok(None),
            1 => Ok(Some(self.read_bool()?)),
            value => bail!("invalid optional bool tag {value} in archive record"),
        }
    }

    fn read_u32(&mut self) -> Result<u32> {
        let bytes = self.read_exact(4)?;
        Ok(u32::from_le_bytes(
            bytes.try_into().expect("slice length checked"),
        ))
    }

    fn read_u64(&mut self) -> Result<u64> {
        let bytes = self.read_exact(8)?;
        Ok(u64::from_le_bytes(
            bytes.try_into().expect("slice length checked"),
        ))
    }

    fn read_i64(&mut self) -> Result<i64> {
        let bytes = self.read_exact(8)?;
        Ok(i64::from_le_bytes(
            bytes.try_into().expect("slice length checked"),
        ))
    }

    fn read_string(&mut self) -> Result<String> {
        let len = self.read_u64()?;
        if len > usize::MAX as u64 {
            bail!("archive string is too large");
        }
        let bytes = self.read_exact(len as usize)?;
        String::from_utf8(bytes.to_vec()).context("archive string is not valid UTF-8")
    }

    fn read_file_stamp_option(&mut self) -> Result<Option<FileStamp>> {
        match self.read_u8()? {
            0 => Ok(None),
            1 => Ok(Some(FileStamp {
                seconds: self.read_i64()?,
                nanos: self.read_u32()?,
            })),
            value => bail!("invalid timestamp option tag {value} in archive record"),
        }
    }

    fn read_unix_metadata_option_fields(&mut self) -> Result<Option<(u32, u32, u32, u64)>> {
        match self.read_u8()? {
            0 => Ok(None),
            1 => Ok(Some((
                self.read_u32()?,
                self.read_u32()?,
                self.read_u32()?,
                self.read_u64()?,
            ))),
            value => bail!("invalid Unix metadata option tag {value} in archive record"),
        }
    }

    fn read_windows_metadata_option_fields(&mut self) -> Result<Option<u32>> {
        match self.read_u8()? {
            0 => Ok(None),
            1 => Ok(Some(self.read_u32()?)),
            value => bail!("invalid Windows metadata option tag {value} in archive record"),
        }
    }
}

fn write_json_record<W: Write>(writer: &mut W, tag: u8, payload: Vec<u8>) -> Result<()> {
    writer.write_all(&[tag])?;
    writer.write_all(&(payload.len() as u64).to_le_bytes())?;
    writer.write_all(&payload)?;
    Ok(())
}

fn read_json_record<R: Read>(reader: &mut R) -> Result<Option<(u8, Vec<u8>)>> {
    let mut tag = [0_u8; 1];
    match reader.read_exact(&mut tag) {
        Ok(()) => {}
        Err(error) if error.kind() == io::ErrorKind::UnexpectedEof => return Ok(None),
        Err(error) => return Err(error).context("failed to read archive record tag"),
    }
    let len = match read_u64(reader) {
        Ok(len) => len,
        Err(error) => bail!("failed to read archive record length: {error}"),
    };
    if len > usize::MAX as u64 {
        bail!("archive record is too large to fit in memory");
    }
    let mut json = vec![0_u8; len as usize];
    reader
        .read_exact(&mut json)
        .context("failed to read archive record payload")?;
    Ok(Some((tag[0], json)))
}

fn read_u64<R: Read>(reader: &mut R) -> Result<u64> {
    let mut bytes = [0_u8; 8];
    reader.read_exact(&mut bytes)?;
    Ok(u64::from_le_bytes(bytes))
}

fn copy_exact_bytes<R: Read, W: Write>(
    reader: &mut R,
    writer: &mut W,
    mut len: u64,
) -> Result<u64> {
    let mut buffer = vec![0_u8; COPY_BUFFER_SIZE];
    let mut copied = 0;
    while len > 0 {
        let chunk_len = buffer.len().min(len as usize);
        let read = reader
            .read(&mut buffer[..chunk_len])
            .context("failed while reading data stream")?;
        if read == 0 {
            bail!("unexpected end of stream while copying file data");
        }
        writer
            .write_all(&buffer[..read])
            .context("failed while writing data stream")?;
        len -= read as u64;
        copied += read as u64;
    }
    Ok(copied)
}
