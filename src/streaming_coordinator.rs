use std::collections::HashSet;
use std::sync::mpsc::{self, Receiver, Sender};
use std::thread;
use std::time::{Duration, Instant};

use crate::config::Config;
use crate::sources::{
    qobuz::QobuzSource,
    song::Song,
    streaming::{
        AuthStatus, ResolvedStream, ResolvedStreamSource, StreamAlbum, StreamArtist, StreamTrack,
        StreamingService, StreamingServiceId,
    },
    tidal::TidalSource,
};

pub enum StreamingRequest {
    Authenticate,
    SearchAlbums {
        query: String,
        limit: u32,
    },
    GetAlbumTracks {
        album_id: String,
        album_title: String,
    },
    PollAuth,
    GetStreamUrl {
        track_id: String,
        title: String,
        enqueue: bool,
        source_song: Option<Box<Song>>,
    },
    /// Resolve stream URLs for an entire album. Resolves the start track first
    /// for immediate playback, then resolves the rest for enqueueing.
    PlayAlbumStream {
        tracks: Vec<StreamTrack>,
        start_index: usize,
    },
    /// Resolve streaming references inside a saved playlist while preserving
    /// already-playable local tracks.
    PlayMixedPlaylist {
        songs: Vec<Song>,
        start_index: usize,
    },
    SearchArtists {
        query: String,
        limit: u32,
    },
    SearchTracks {
        query: String,
        limit: u32,
    },
    GetArtistAlbums {
        artist_id: String,
        artist_name: String,
    },
}

impl StreamingRequest {
    fn kind(&self) -> StreamingTaskKind {
        match self {
            Self::Authenticate => StreamingTaskKind::Authenticate,
            Self::SearchAlbums { .. } => StreamingTaskKind::Search,
            Self::GetAlbumTracks { .. } => StreamingTaskKind::GetAlbumTracks,
            Self::PollAuth => StreamingTaskKind::PollAuth,
            Self::GetStreamUrl { .. } => StreamingTaskKind::GetStreamUrl,
            Self::PlayAlbumStream { .. } => StreamingTaskKind::PlayAlbumStream,
            Self::PlayMixedPlaylist { .. } => StreamingTaskKind::PlayAlbumStream,
            Self::SearchArtists { .. } => StreamingTaskKind::SearchArtists,
            Self::SearchTracks { .. } => StreamingTaskKind::SearchTracks,
            Self::GetArtistAlbums { .. } => StreamingTaskKind::GetArtistAlbums,
        }
    }

    fn is_search(&self) -> bool {
        matches!(
            self,
            Self::SearchAlbums { .. } | Self::SearchArtists { .. } | Self::SearchTracks { .. }
        )
    }

    fn status(&self, service_id: StreamingServiceId) -> String {
        match self {
            Self::Authenticate => format!("Authenticating {}...", service_id.as_str()),
            Self::SearchAlbums { .. } => format!("Searching {}...", service_id.as_str()),
            Self::GetAlbumTracks { .. } => "Loading album tracks...".to_string(),
            Self::PollAuth => format!("Authenticating {}...", service_id.as_str()),
            Self::GetStreamUrl { .. } => "Loading stream...".to_string(),
            Self::PlayAlbumStream { .. } => "Resolving album streams...".to_string(),
            Self::PlayMixedPlaylist { .. } => "Resolving playlist streams...".to_string(),
            Self::SearchArtists { .. } => {
                format!("Searching {} artists...", service_id.as_str())
            }
            Self::SearchTracks { .. } => format!("Searching {} tracks...", service_id.as_str()),
            Self::GetArtistAlbums { .. } => "Loading artist albums...".to_string(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum StreamingTaskKind {
    Authenticate,
    Search,
    GetAlbumTracks,
    PollAuth,
    GetStreamUrl,
    PlayAlbumStream,
    SearchArtists,
    SearchTracks,
    GetArtistAlbums,
}

impl StreamingTaskKind {
    fn is_search(self) -> bool {
        matches!(
            self,
            Self::Search | Self::SearchArtists | Self::SearchTracks
        )
    }
}

#[derive(Debug, Clone, Copy)]
struct ActiveStreamingTask {
    id: u64,
    service: StreamingServiceId,
    kind: StreamingTaskKind,
    started_at: Instant,
    timeout: Duration,
}

pub enum StreamingTaskOutput {
    AlbumSearchResults(Vec<StreamAlbum>),
    AlbumTracks {
        album_title: String,
        tracks: Vec<StreamTrack>,
    },
    AuthPending {
        message: String,
        deferred_query: Option<String>,
    },
    AuthCompleted,
    PollPending,
    StreamUrlResult {
        title: String,
        stream: Option<ResolvedStream>,
        enqueue: bool,
        source_song: Option<Box<Song>>,
    },
    AlbumStreamUrls {
        first_song: Option<Song>,
        remaining_songs: Vec<Song>,
        failed_count: usize,
    },
    ArtistSearchResults(Vec<StreamArtist>),
    TrackSearchResults(Vec<StreamTrack>),
    ArtistAlbums {
        artist_name: String,
        albums: Vec<StreamAlbum>,
    },
    Error(String),
}

struct StreamingTaskResult {
    task_id: u64,
    service_name: StreamingServiceId,
    service: Box<dyn StreamingService>,
    output: StreamingTaskOutput,
}

pub enum StreamingCoordinatorEvent {
    Status(Option<String>),
    ServiceReturned(StreamingServiceId),
    Output {
        service_id: StreamingServiceId,
        output: Box<StreamingTaskOutput>,
    },
    TimedOut {
        service_id: StreamingServiceId,
        timeout: Duration,
    },
}

pub enum StreamingSubmitResult {
    Started { status: String },
    ReplacedSearch { status: String },
    Queued { status: String },
    Unavailable { status: String },
    Busy,
}

pub struct StreamingCoordinator {
    qobuz: Option<Box<dyn StreamingService>>,
    tidal: Option<Box<dyn StreamingService>>,
    task_tx: Sender<StreamingTaskResult>,
    task_rx: Receiver<StreamingTaskResult>,
    busy_service: Option<StreamingServiceId>,
    next_task_id: u64,
    active_task: Option<ActiveStreamingTask>,
    canceled_task_ids: HashSet<u64>,
    discarded_task_ids: HashSet<u64>,
    recovering_service: Option<StreamingServiceId>,
    queued_search: Option<(StreamingServiceId, StreamingRequest)>,
    search_task_timeout: Duration,
    auth_task_timeout: Duration,
    stream_url_task_timeout: Duration,
}

impl StreamingCoordinator {
    pub fn new(
        qobuz: Option<Box<dyn StreamingService>>,
        tidal: Option<Box<dyn StreamingService>>,
    ) -> Self {
        let (task_tx, task_rx) = mpsc::channel();

        Self {
            qobuz,
            tidal,
            task_tx,
            task_rx,
            busy_service: None,
            next_task_id: 1,
            active_task: None,
            canceled_task_ids: HashSet::new(),
            discarded_task_ids: HashSet::new(),
            recovering_service: None,
            queued_search: None,
            search_task_timeout: Duration::from_secs(20),
            auth_task_timeout: Duration::from_secs(10),
            stream_url_task_timeout: Duration::from_secs(20),
        }
    }

    pub fn set_timeouts(
        &mut self,
        search_timeout: Duration,
        auth_timeout: Duration,
        stream_url_timeout: Duration,
    ) {
        self.search_task_timeout = search_timeout;
        self.auth_task_timeout = auth_timeout;
        self.stream_url_task_timeout = stream_url_timeout;
    }

    pub fn is_busy(&self, service_id: StreamingServiceId) -> bool {
        self.busy_service == Some(service_id)
    }

    pub fn can_submit_search(&self, service_id: StreamingServiceId) -> bool {
        self.service_ref(service_id).is_some()
            || self.busy_service == Some(service_id)
            || self.recovering_service == Some(service_id)
    }

    pub fn replace_qobuz(&mut self, qobuz: Option<Box<dyn StreamingService>>) {
        self.qobuz = qobuz;
    }

    pub fn replace_tidal(&mut self, tidal: Option<Box<dyn StreamingService>>) {
        self.tidal = tidal;
    }

    pub fn reset_service(
        &mut self,
        service_id: StreamingServiceId,
        replacement: Option<Box<dyn StreamingService>>,
    ) {
        if let Some(active) = self.active_task {
            if active.service == service_id {
                self.discarded_task_ids.insert(active.id);
                self.active_task = None;
                self.busy_service = None;
            }
        }

        let _ = self.take_service(service_id);

        if self.recovering_service == Some(service_id) {
            self.recovering_service = None;
        }

        if self
            .queued_search
            .as_ref()
            .is_some_and(|(queued_service, _)| *queued_service == service_id)
        {
            self.queued_search = None;
        }

        if let Some(service) = replacement {
            self.put_service(service_id, service);
        }
    }

    pub fn persist_data(&self, service_id: StreamingServiceId) -> Option<String> {
        self.service_ref(service_id)
            .and_then(|service| service.persist_data())
    }

    pub fn app_credentials(&self, service_id: StreamingServiceId) -> Option<(String, String)> {
        self.service_ref(service_id)
            .and_then(|service| service.app_credentials())
    }

    pub fn submit(
        &mut self,
        service_id: StreamingServiceId,
        request: StreamingRequest,
        config: &Config,
    ) -> StreamingSubmitResult {
        if self.busy_service == Some(service_id) {
            if let Some(active) = self.active_task {
                if active.service == service_id && active.kind.is_search() && request.is_search() {
                    self.cancel_active_task(active, config);
                    let status = self.spawn_task_for_available_service(service_id, request);
                    return StreamingSubmitResult::ReplacedSearch { status };
                }
            }
            return StreamingSubmitResult::Busy;
        }

        if self.service_ref(service_id).is_none() {
            if request.is_search() && self.recovering_service == Some(service_id) {
                self.queued_search = Some((service_id, request));
                return StreamingSubmitResult::Queued {
                    status: "Waiting for previous request cleanup...".to_string(),
                };
            }

            return StreamingSubmitResult::Unavailable {
                status: format!("Configure {} in Settings", service_id.as_str()),
            };
        }

        let status = self.spawn_task_for_available_service(service_id, request);
        StreamingSubmitResult::Started { status }
    }

    pub fn check_timeouts(&mut self, config: &Config) -> Vec<StreamingCoordinatorEvent> {
        let Some(active) = self.active_task else {
            return Vec::new();
        };
        if active.started_at.elapsed() <= active.timeout {
            return Vec::new();
        }
        if self.canceled_task_ids.contains(&active.id) {
            return Vec::new();
        }

        self.cancel_active_task(active, config);
        vec![
            StreamingCoordinatorEvent::Status(Some(format!(
                "{} request timed out; cleaning up...",
                active.service.as_str()
            ))),
            StreamingCoordinatorEvent::TimedOut {
                service_id: active.service,
                timeout: active.timeout,
            },
        ]
    }

    pub fn poll_events(&mut self) -> Vec<StreamingCoordinatorEvent> {
        let mut events = Vec::new();

        while let Ok(result) = self.task_rx.try_recv() {
            self.handle_task_result(result, &mut events);
        }
        while self.busy_service.is_some() {
            match self.task_rx.recv_timeout(Duration::from_millis(20)) {
                Ok(result) => self.handle_task_result(result, &mut events),
                Err(_) => break,
            }
        }

        events
    }

    fn service_ref(&self, service_id: StreamingServiceId) -> Option<&dyn StreamingService> {
        match service_id {
            StreamingServiceId::Qobuz => self.qobuz.as_deref(),
            StreamingServiceId::Tidal => self.tidal.as_deref(),
        }
    }

    fn take_service(
        &mut self,
        service_id: StreamingServiceId,
    ) -> Option<Box<dyn StreamingService>> {
        match service_id {
            StreamingServiceId::Qobuz => self.qobuz.take(),
            StreamingServiceId::Tidal => self.tidal.take(),
        }
    }

    fn put_service(&mut self, service_id: StreamingServiceId, service: Box<dyn StreamingService>) {
        match service_id {
            StreamingServiceId::Qobuz => self.qobuz = Some(service),
            StreamingServiceId::Tidal => self.tidal = Some(service),
        }
    }

    fn spawn_task_for_available_service(
        &mut self,
        service_id: StreamingServiceId,
        request: StreamingRequest,
    ) -> String {
        let status = request.status(service_id);
        if let Some(service) = self.take_service(service_id) {
            self.spawn_task(service_id, service, request);
        }
        status
    }

    fn spawn_task(
        &mut self,
        service_id: StreamingServiceId,
        mut service: Box<dyn StreamingService>,
        request: StreamingRequest,
    ) {
        let kind = request.kind();
        let timeout = match &request {
            StreamingRequest::Authenticate | StreamingRequest::PollAuth => self.auth_task_timeout,
            StreamingRequest::SearchAlbums { .. }
            | StreamingRequest::GetAlbumTracks { .. }
            | StreamingRequest::SearchArtists { .. }
            | StreamingRequest::SearchTracks { .. }
            | StreamingRequest::GetArtistAlbums { .. } => self.search_task_timeout,
            StreamingRequest::GetStreamUrl { .. } => self.stream_url_task_timeout,
            StreamingRequest::PlayAlbumStream { tracks, .. } => {
                Duration::from_secs(self.stream_url_task_timeout.as_secs() * tracks.len() as u64)
            }
            StreamingRequest::PlayMixedPlaylist { songs, .. } => {
                let stream_count = songs
                    .iter()
                    .filter(|song| song.has_stream_reference())
                    .count();
                Duration::from_secs(
                    self.stream_url_task_timeout.as_secs() * stream_count.max(1) as u64,
                )
            }
        };
        let task_id = self.next_task_id;
        self.next_task_id = self.next_task_id.saturating_add(1);
        self.busy_service = Some(service_id);
        self.active_task = Some(ActiveStreamingTask {
            id: task_id,
            service: service_id,
            kind,
            started_at: Instant::now(),
            timeout,
        });
        let tx = self.task_tx.clone();
        thread::spawn(move || {
            let output = execute_request(service_id, &mut service, request);
            let _ = tx.send(StreamingTaskResult {
                task_id,
                service_name: service_id,
                service,
                output,
            });
        });
    }

    fn handle_task_result(
        &mut self,
        result: StreamingTaskResult,
        events: &mut Vec<StreamingCoordinatorEvent>,
    ) {
        let was_active = self
            .active_task
            .map(|a| a.id == result.task_id)
            .unwrap_or(false);
        let is_canceled = self.canceled_task_ids.remove(&result.task_id);
        let is_discarded = self.discarded_task_ids.remove(&result.task_id);
        if was_active {
            self.active_task = None;
            self.busy_service = None;
            events.push(StreamingCoordinatorEvent::Status(None));
        }

        let service_id = result.service_name;
        if is_discarded {
            return;
        }

        self.put_service(service_id, result.service);
        if self.recovering_service == Some(service_id) {
            self.recovering_service = None;
        }
        events.push(StreamingCoordinatorEvent::ServiceReturned(service_id));

        if !is_canceled {
            events.push(StreamingCoordinatorEvent::Output {
                service_id,
                output: Box::new(result.output),
            });
        }

        if let Some(status) = self.maybe_start_queued_search_for(service_id) {
            events.push(StreamingCoordinatorEvent::Status(Some(status)));
        }
    }

    fn cancel_active_task(&mut self, active: ActiveStreamingTask, config: &Config) {
        self.canceled_task_ids.insert(active.id);
        self.active_task = None;
        self.busy_service = None;

        let _ = self.take_service(active.service);
        if let Some(service) = Self::recreate_service(active.service, config) {
            self.put_service(active.service, service);
        } else {
            self.recovering_service = Some(active.service);
        }
    }

    fn recreate_service(
        service_id: StreamingServiceId,
        config: &Config,
    ) -> Option<Box<dyn StreamingService>> {
        match service_id {
            StreamingServiceId::Qobuz => {
                config
                    .qobuz
                    .as_ref()
                    .filter(|q| q.has_credentials())
                    .map(|q| {
                        Box::new(QobuzSource::with_credentials(
                            q.app_id.clone(),
                            q.app_secret.clone(),
                            q.email.clone(),
                            q.password.clone(),
                            config.audio.max_stream_quality,
                        )) as Box<dyn StreamingService>
                    })
            }
            StreamingServiceId::Tidal => Some(Box::new(TidalSource::new(
                config.tidal.clone().unwrap_or_default(),
                config.audio.max_stream_quality,
            )) as Box<dyn StreamingService>),
        }
    }

    fn maybe_start_queued_search_for(&mut self, service_id: StreamingServiceId) -> Option<String> {
        if self.busy_service.is_some() {
            return None;
        }
        let queued = self.queued_search.take();
        match queued {
            Some((queued_service, request)) if queued_service == service_id => {
                Some(self.spawn_task_for_available_service(service_id, request))
            }
            Some(other) => {
                self.queued_search = Some(other);
                None
            }
            None => None,
        }
    }
}

fn execute_request(
    service_id: StreamingServiceId,
    service: &mut Box<dyn StreamingService>,
    request: StreamingRequest,
) -> StreamingTaskOutput {
    match request {
        StreamingRequest::Authenticate => match service.authenticate() {
            Ok(AuthStatus::Authenticated) => StreamingTaskOutput::AuthCompleted,
            Ok(AuthStatus::PendingUserAction(message)) => StreamingTaskOutput::AuthPending {
                message,
                deferred_query: None,
            },
            Err(e) => StreamingTaskOutput::Error(format!("Auth failed: {}", e)),
        },
        StreamingRequest::SearchAlbums { query, limit } => {
            execute_authenticated_search(service, query, |service, query| {
                match service.search_albums(&query, limit) {
                    Ok(albums) => StreamingTaskOutput::AlbumSearchResults(albums),
                    Err(e) => StreamingTaskOutput::Error(format!("Search failed: {}", e)),
                }
            })
        }
        StreamingRequest::GetAlbumTracks {
            album_id,
            album_title,
        } => match service.get_album_tracks(&album_id) {
            Ok(tracks) => StreamingTaskOutput::AlbumTracks {
                album_title,
                tracks,
            },
            Err(e) => StreamingTaskOutput::Error(format!("Failed to load album: {}", e)),
        },
        StreamingRequest::PollAuth => match service.poll_auth() {
            Ok(true) => StreamingTaskOutput::AuthCompleted,
            Ok(false) => StreamingTaskOutput::PollPending,
            Err(e) => StreamingTaskOutput::Error(format!("Auth polling failed: {}", e)),
        },
        StreamingRequest::GetStreamUrl {
            track_id,
            title,
            enqueue,
            source_song,
        } => match service.get_stream_url(&track_id) {
            Ok(stream) => StreamingTaskOutput::StreamUrlResult {
                title,
                stream,
                enqueue,
                source_song,
            },
            Err(e) => StreamingTaskOutput::Error(format!("Stream URL error: {}", e)),
        },
        StreamingRequest::PlayAlbumStream {
            tracks,
            start_index,
        } => resolve_album_streams(service_id, service, tracks, start_index),
        StreamingRequest::PlayMixedPlaylist { songs, start_index } => {
            resolve_mixed_playlist_streams(service, songs, start_index)
        }
        StreamingRequest::SearchArtists { query, limit } => {
            execute_authenticated_search(service, query, |service, query| {
                match service.search_artists(&query, limit) {
                    Ok(artists) => StreamingTaskOutput::ArtistSearchResults(artists),
                    Err(e) => StreamingTaskOutput::Error(format!("Artist search failed: {}", e)),
                }
            })
        }
        StreamingRequest::SearchTracks { query, limit } => {
            execute_authenticated_search(service, query, |service, query| {
                match service.search(&query, limit) {
                    Ok(tracks) => StreamingTaskOutput::TrackSearchResults(tracks),
                    Err(e) => StreamingTaskOutput::Error(format!("Track search failed: {}", e)),
                }
            })
        }
        StreamingRequest::GetArtistAlbums {
            artist_id,
            artist_name,
        } => match service.get_artist_albums(&artist_id) {
            Ok(albums) => StreamingTaskOutput::ArtistAlbums {
                artist_name,
                albums,
            },
            Err(e) => StreamingTaskOutput::Error(format!("Failed to load artist albums: {}", e)),
        },
    }
}

fn execute_authenticated_search(
    service: &mut Box<dyn StreamingService>,
    query: String,
    search: impl FnOnce(&mut Box<dyn StreamingService>, String) -> StreamingTaskOutput,
) -> StreamingTaskOutput {
    if !service.is_authenticated() {
        match service.authenticate() {
            Ok(AuthStatus::Authenticated) => search(service, query),
            Ok(AuthStatus::PendingUserAction(message)) => StreamingTaskOutput::AuthPending {
                message,
                deferred_query: Some(query),
            },
            Err(e) => StreamingTaskOutput::Error(format!("Auth failed: {}", e)),
        }
    } else {
        search(service, query)
    }
}

fn song_from_resolved_stream(
    service_id: StreamingServiceId,
    track: &StreamTrack,
    stream: ResolvedStream,
) -> Song {
    let ResolvedStream {
        source,
        quality_label,
    } = stream;
    let mut song = match source {
        ResolvedStreamSource::Url(url) => Song::from_url(track.title.clone(), url, quality_label),
        ResolvedStreamSource::Manifest {
            contents,
            file_extension,
        } => Song::from_manifest(track.title.clone(), contents, file_extension, quality_label),
    };

    song.artist = track.artist.clone();
    song.album_name = track.album.clone();
    song.stream_service = Some(service_id.as_str().to_string());
    song.stream_track_id = Some(track.id.clone());
    song
}

fn song_from_playlist_stream(song: &Song, stream: ResolvedStream) -> Song {
    let ResolvedStream {
        source,
        quality_label,
    } = stream;

    let mut resolved = match source {
        ResolvedStreamSource::Url(url) => Song::from_url(song.title.clone(), url, quality_label),
        ResolvedStreamSource::Manifest {
            contents,
            file_extension,
        } => Song::from_manifest(song.title.clone(), contents, file_extension, quality_label),
    };

    resolved.artist = song.artist.clone();
    resolved.album_name = song.album_name.clone();
    resolved.disc_number = song.disc_number;
    resolved.track_number = song.track_number;
    resolved.duration_secs = song.duration_secs;
    resolved.stream_service = song.stream_service.clone();
    resolved.stream_track_id = song.stream_track_id.clone();
    resolved
}

fn resolve_album_streams(
    service_id: StreamingServiceId,
    service: &mut Box<dyn StreamingService>,
    tracks: Vec<StreamTrack>,
    start_index: usize,
) -> StreamingTaskOutput {
    let mut failed_count = 0;
    let mut resolved: Vec<Option<Song>> = Vec::with_capacity(tracks.len());

    for (i, track) in tracks.iter().enumerate() {
        if i == start_index {
            match service.get_stream_url(&track.id) {
                Ok(Some(stream)) => {
                    resolved.push(Some(song_from_resolved_stream(service_id, track, stream)))
                }
                _ => {
                    failed_count += 1;
                    resolved.push(None);
                }
            }
        } else {
            resolved.push(None);
        }
    }

    for (i, track) in tracks.iter().enumerate() {
        if i == start_index {
            continue;
        }
        match service.get_stream_url(&track.id) {
            Ok(Some(stream)) => {
                resolved[i] = Some(song_from_resolved_stream(service_id, track, stream));
            }
            _ => {
                failed_count += 1;
            }
        }
    }

    let first_song = resolved[start_index].take();
    let mut remaining_songs = Vec::new();
    for song in resolved
        .iter_mut()
        .skip(start_index + 1)
        .filter_map(Option::take)
    {
        remaining_songs.push(song);
    }
    for song in resolved
        .iter_mut()
        .take(start_index)
        .filter_map(Option::take)
    {
        remaining_songs.push(song);
    }

    StreamingTaskOutput::AlbumStreamUrls {
        first_song,
        remaining_songs,
        failed_count,
    }
}

fn resolve_mixed_playlist_streams(
    service: &mut Box<dyn StreamingService>,
    songs: Vec<Song>,
    start_index: usize,
) -> StreamingTaskOutput {
    if start_index >= songs.len() {
        return StreamingTaskOutput::Error("Playlist start index out of bounds".to_string());
    }

    let mut failed_count = 0;
    let mut resolved: Vec<Option<Song>> = Vec::with_capacity(songs.len());

    for song in &songs {
        if let Some(track_id) = song.stream_track_id.as_deref() {
            match service.get_stream_url(track_id) {
                Ok(Some(stream)) => resolved.push(Some(song_from_playlist_stream(song, stream))),
                _ => {
                    failed_count += 1;
                    resolved.push(None);
                }
            }
        } else {
            resolved.push(Some(song.clone()));
        }
    }

    let first_song = resolved[start_index].take();
    let mut remaining_songs = Vec::new();
    for song in resolved
        .iter_mut()
        .skip(start_index + 1)
        .filter_map(Option::take)
    {
        remaining_songs.push(song);
    }
    for song in resolved
        .iter_mut()
        .take(start_index)
        .filter_map(Option::take)
    {
        remaining_songs.push(song);
    }

    StreamingTaskOutput::AlbumStreamUrls {
        first_song,
        remaining_songs,
        failed_count,
    }
}
