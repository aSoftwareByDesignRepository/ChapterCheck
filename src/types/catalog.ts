export type AppView = "home" | "audiobooks" | "music" | "playlists" | "library" | "nowPlaying";

export type CollectionSummaryDto = {
  id: number;
  root_id: number;
  kind: string;
  title: string;
  subtitle: string | null;
  layout_kind: string;
  cover_path: string | null;
  progress_pct: number;
  listened: boolean;
  in_progress: boolean;
  unavailable: boolean;
  root_unavailable: boolean;
  playable_file_count: number;
  missing_file_count: number;
  location_hint: string;
  track_count: number;
  last_played_at: number | null;
};

export type CollectionDetailDto = {
  id: number;
  root_id: number;
  kind: string;
  title: string;
  author: string | null;
  narrator: string | null;
  artist: string | null;
  album: string | null;
  series: string | null;
  series_index: number | null;
  layout_kind: string;
  cover_path: string | null;
  progress_pct: number;
  listened: boolean;
  unavailable: boolean;
  root_unavailable: boolean;
  missing_file_count: number;
  playable_file_count: number;
  location_hint: string;
  is_manual: boolean;
  files: CollectionFileDto[];
};

export type CollectionFileDto = {
  id: number;
  path: string;
  display_title: string;
  label: string;
  track_order: number;
  disc_index: number;
  track_index: number;
  duration_sec: number | null;
  position_sec: number;
  listened: boolean;
  unavailable: boolean;
};

export type HomeSummaryDto = {
  continue_item: CollectionSummaryDto | null;
  in_progress: CollectionSummaryDto[];
  music_shelf: CollectionSummaryDto[];
  has_library: boolean;
  scan_in_progress: boolean;
};

export type LibraryRootDto = {
  id: number;
  path: string;
  label: string;
  content_kind: string;
  scan_rule: string;
  scan_subfolders: boolean;
  is_available: boolean;
  last_scan_at: number | null;
  last_scan_status: string | null;
  collection_count: number;
};

export type PlaylistSummaryDto = {
  id: number;
  name: string;
  kind: string;
  is_pinned: boolean;
  track_count: number;
  unavailable_count: number;
};

export type PlaylistItemDto = {
  id: number;
  collection_file_id: number;
  track_order: number;
  display_title: string;
  collection_title: string;
  unavailable: boolean;
};

export type MetadataSuggestionDto = {
  title: string | null;
  author: string | null;
  narrator: string | null;
  artist: string | null;
  album: string | null;
  source: string;
};

export type PlaylistDetailDto = {
  id: number;
  name: string;
  kind: string;
  is_pinned: boolean;
  default_playback_speed: number | null;
  items: PlaylistItemDto[];
};

export type ImportFolderToPlaylistResult = {
  folder_path: string;
  tracks_added: number;
  tracks_skipped: number;
  tracks_total: number;
  library_linked: boolean;
};

export type AlbumGroupDto = {
  artist: string;
  album: string;
  track_count: number;
};

export type MetadataGroupKind =
  | "album"
  | "artist"
  | "audiobook"
  | "author"
  | "narrator"
  | "series";

export type MetadataGroupDto = {
  group_kind: MetadataGroupKind;
  group_key: string;
  label: string;
  subtitle: string | null;
  track_count: number;
};

export type AddToPlaylistBulkResult = {
  tracks_added: number;
  tracks_skipped: number;
};

export type RemoveCollectionFileResult = {
  collection_removed: boolean;
  removed_path: string;
};

export type RemoveCollectionResult = {
  removed_paths: string[];
};
