//! A host filesystem spy: every filesystem call must carry the original policy.
use codex_exec_server::LocalFileSystem;
use codex_file_system::*;
use codex_utils_path_uri::PathUri;

pub(super) struct CheckedFileSystem {
    pub sandbox: FileSystemSandboxContext,
    pub walk_truncated: bool,
    pub metadata_error: Option<String>,
    pub metadata_panic: bool,
    inner: LocalFileSystem,
}

impl CheckedFileSystem {
    pub fn new(sandbox: FileSystemSandboxContext) -> Self {
        Self {
            sandbox,
            walk_truncated: false,
            metadata_error: None,
            metadata_panic: false,
            inner: LocalFileSystem::unsandboxed(),
        }
    }
}

macro_rules! delegate {
    ($name:ident ($($argument:ident: $type:ty),*) -> $output:ty) => {
        fn $name<'a>(&'a self, $($argument: $type,)* sandbox: Option<&'a FileSystemSandboxContext>) -> ExecutorFileSystemFuture<'a, $output> {
            assert_eq!(sandbox, Some(&self.sandbox));
            self.inner.$name($($argument,)* sandbox)
        }
    };
}

impl ExecutorFileSystem for CheckedFileSystem {
    delegate!(canonicalize(path: &'a PathUri) -> PathUri);
    delegate!(read_file(path: &'a PathUri, options: ReadFileOptions) -> Vec<u8>);
    delegate!(read_file_stream(path: &'a PathUri) -> FileSystemReadStream);
    delegate!(write_file(path: &'a PathUri, contents: Vec<u8>, options: WriteFileOptions) -> ());
    delegate!(create_directory(path: &'a PathUri, options: CreateDirectoryOptions) -> ());
    delegate!(read_directory(path: &'a PathUri) -> Vec<ReadDirectoryEntry>);
    delegate!(remove(path: &'a PathUri, options: RemoveOptions) -> ());
    delegate!(copy(source: &'a PathUri, destination: &'a PathUri, options: CopyOptions) -> ());

    fn get_metadata<'a>(
        &'a self,
        path: &'a PathUri,
        options: GetMetadataOptions,
        sandbox: Option<&'a FileSystemSandboxContext>,
    ) -> ExecutorFileSystemFuture<'a, FileMetadata> {
        assert_eq!(sandbox, Some(&self.sandbox));
        if self.metadata_panic {
            return Box::pin(async { panic!("Synthetic filesystem panic after tool started") });
        }
        if let Some(message) = &self.metadata_error {
            return Box::pin(async move { Err(std::io::Error::other(message.clone())) });
        }
        self.inner.get_metadata(path, options, sandbox)
    }

    fn walk<'a>(
        &'a self,
        path: &'a PathUri,
        options: WalkOptions,
        sandbox: Option<&'a FileSystemSandboxContext>,
    ) -> ExecutorFileSystemFuture<'a, WalkOutcome> {
        assert_eq!(sandbox, Some(&self.sandbox));
        assert!(!options.follow_directory_symlinks);
        Box::pin(async move {
            let mut result = self.inner.walk(path, options, sandbox).await?;
            result.truncated |= self.walk_truncated;
            Ok(result)
        })
    }
}
