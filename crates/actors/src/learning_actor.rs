//! LearningActor — manages tracks and lesson delivery.
//!
//! State: in-memory HashMap of tracks (hot cache). Lessons are fetched
//! from DB on-demand (they can be large).
//!
//! CheckDue is invoked by the scheduler tick in bootstrap — scans all tracks
//! and delivers next lesson if `now - last_delivered_at >= pace_hours`.
//! Delivery = mark lesson as `Delivered`, return it to caller so the caller
//! (e.g. bootstrap tick handler) can create a Reminder and a Note from it.

use std::collections::HashMap;
use std::sync::Arc;

use chrono::{Duration, Utc};
use ractor::{Actor, ActorProcessingErr, ActorRef, RpcReplyPort};

use application::ports::{
    LearningTrackRepository, LessonRepository, LessonSplitter,
};
use domain::entities::{LearningTrack, Lesson, LessonStatus};

pub enum LearningMsg {
    /// Create a track: split raw_text into lessons via LLM, persist.
    CreateTrack(
        String,          // title
        String,          // source_ref
        String,          // raw_text (full material)
        i32,             // pace_hours
        Vec<String>,     // tags
        RpcReplyPort<Result<LearningTrack, String>>,
    ),
    ListTracks(RpcReplyPort<Vec<LearningTrack>>),
    GetTrack(String, RpcReplyPort<Option<LearningTrack>>),
    DeleteTrack(String, RpcReplyPort<Result<(), String>>),
    ListLessons(String, RpcReplyPort<Vec<Lesson>>),
    /// Manually deliver next lesson for a track.
    DeliverNext(String, RpcReplyPort<Result<Option<Lesson>, String>>),
    MarkLearned(String, RpcReplyPort<Result<(), String>>),
    SkipLesson(String, RpcReplyPort<Result<(), String>>),
    /// Scheduler tick — checks all tracks, delivers overdue ones.
    /// Returns list of freshly-delivered lessons so the caller can wire them
    /// into reminders/notes.
    CheckDue(RpcReplyPort<Vec<Lesson>>),
}

pub struct LearningActor {
    pub track_repo: Arc<dyn LearningTrackRepository>,
    pub lesson_repo: Arc<dyn LessonRepository>,
    pub splitter: Arc<dyn LessonSplitter>,
}

impl Actor for LearningActor {
    type Msg = LearningMsg;
    type State = HashMap<String, LearningTrack>;
    type Arguments = ();

    async fn pre_start(
        &self,
        _: ActorRef<Self::Msg>,
        _: Self::Arguments,
    ) -> Result<Self::State, ActorProcessingErr> {
        let all = self.track_repo.list().await.unwrap_or_default();
        let mut map = HashMap::with_capacity(all.len());
        for t in all {
            map.insert(t.id.clone(), t);
        }
        tracing::info!(count = map.len(), "learning actor: loaded tracks from db");
        Ok(map)
    }

    async fn handle(
        &self,
        _: ActorRef<Self::Msg>,
        msg: Self::Msg,
        state: &mut Self::State,
    ) -> Result<(), ActorProcessingErr> {
        match msg {
            LearningMsg::CreateTrack(title, source_ref, raw_text, pace_hours, tags, reply) => {
                let result = self
                    .create_track_impl(title, source_ref, raw_text, pace_hours, tags, state)
                    .await;
                let _ = reply.send(result);
            }
            LearningMsg::ListTracks(reply) => {
                let mut all: Vec<LearningTrack> = state.values().cloned().collect();
                all.sort_by(|a, b| b.created_at.cmp(&a.created_at));
                let _ = reply.send(all);
            }
            LearningMsg::GetTrack(id, reply) => {
                let _ = reply.send(state.get(&id).cloned());
            }
            LearningMsg::DeleteTrack(id, reply) => {
                let result = match self.track_repo.delete(&id).await {
                    Ok(()) => {
                        state.remove(&id);
                        tracing::info!(%id, "learning track deleted");
                        Ok(())
                    }
                    Err(e) => Err(e.to_string()),
                };
                let _ = reply.send(result);
            }
            LearningMsg::ListLessons(track_id, reply) => {
                let lessons = self.lesson_repo.list_by_track(&track_id).await.unwrap_or_default();
                let _ = reply.send(lessons);
            }
            LearningMsg::DeliverNext(track_id, reply) => {
                let result = self.deliver_next_impl(&track_id, state).await;
                let _ = reply.send(result);
            }
            LearningMsg::MarkLearned(id, reply) => {
                let result = self.update_lesson_status(&id, LessonStatus::Learned).await;
                let _ = reply.send(result);
            }
            LearningMsg::SkipLesson(id, reply) => {
                let result = self.update_lesson_status(&id, LessonStatus::Skipped).await;
                let _ = reply.send(result);
            }
            LearningMsg::CheckDue(reply) => {
                let now = Utc::now();
                let mut delivered = Vec::new();
                let track_ids: Vec<String> = state.keys().cloned().collect();
                for track_id in track_ids {
                    let track = match state.get(&track_id).cloned() {
                        Some(t) => t,
                        None => continue,
                    };
                    let overdue = match track.last_delivered_at {
                        None => true,
                        Some(last) => {
                            now - last >= Duration::hours(track.pace_hours as i64)
                        }
                    };
                    if !overdue {
                        continue;
                    }
                    match self.deliver_next_impl(&track_id, state).await {
                        Ok(Some(lesson)) => delivered.push(lesson),
                        Ok(None) => {
                            tracing::debug!(%track_id, "no more pending lessons");
                        }
                        Err(e) => {
                            tracing::warn!(%track_id, error = %e, "deliver_next failed in CheckDue");
                        }
                    }
                }
                let _ = reply.send(delivered);
            }
        }
        Ok(())
    }
}

impl LearningActor {
    async fn create_track_impl(
        &self,
        title: String,
        source_ref: String,
        raw_text: String,
        pace_hours: i32,
        tags: Vec<String>,
        state: &mut HashMap<String, LearningTrack>,
    ) -> Result<LearningTrack, String> {
        // 1. Split via LLM
        let drafts = self
            .splitter
            .split(&title, &raw_text)
            .await
            .map_err(|e| format!("splitter: {e}"))?;

        if drafts.is_empty() {
            return Err("splitter returned 0 lessons".into());
        }

        // 2. Create track
        let mut track = LearningTrack::new(title, source_ref, pace_hours, tags);
        track.total_lessons = drafts.len() as i32;

        self.track_repo
            .upsert(&track)
            .await
            .map_err(|e| e.to_string())?;

        // 3. Create lessons
        for (idx, draft) in drafts.into_iter().enumerate() {
            let lesson = Lesson::new(
                track.id.clone(),
                (idx + 1) as i32,
                draft.title,
                draft.content,
            );
            self.lesson_repo
                .upsert(&lesson)
                .await
                .map_err(|e| e.to_string())?;
        }

        tracing::info!(
            id = %track.id,
            title = %track.title,
            total = track.total_lessons,
            "learning track created"
        );

        state.insert(track.id.clone(), track.clone());
        Ok(track)
    }

    async fn deliver_next_impl(
        &self,
        track_id: &str,
        state: &mut HashMap<String, LearningTrack>,
    ) -> Result<Option<Lesson>, String> {
        let mut track = state
            .get(track_id)
            .cloned()
            .ok_or_else(|| format!("track not found: {track_id}"))?;

        let lesson = self
            .lesson_repo
            .next_pending(track_id)
            .await
            .map_err(|e| e.to_string())?;
        let Some(mut lesson) = lesson else {
            return Ok(None);
        };

        lesson.status = LessonStatus::Delivered;
        lesson.delivered_at = Some(Utc::now());
        self.lesson_repo
            .upsert(&lesson)
            .await
            .map_err(|e| e.to_string())?;

        track.current_lesson = lesson.lesson_num;
        track.last_delivered_at = Some(Utc::now());
        self.track_repo
            .upsert(&track)
            .await
            .map_err(|e| e.to_string())?;
        state.insert(track.id.clone(), track);

        tracing::info!(
            lesson_id = %lesson.id,
            track_id = %track_id,
            num = lesson.lesson_num,
            "lesson delivered"
        );
        Ok(Some(lesson))
    }

    async fn update_lesson_status(
        &self,
        lesson_id: &str,
        status: LessonStatus,
    ) -> Result<(), String> {
        let mut lesson = self
            .lesson_repo
            .get(lesson_id)
            .await
            .map_err(|e| e.to_string())?
            .ok_or_else(|| format!("lesson not found: {lesson_id}"))?;
        lesson.status = status;
        if status == LessonStatus::Learned {
            lesson.learned_at = Some(Utc::now());
        }
        self.lesson_repo
            .upsert(&lesson)
            .await
            .map_err(|e| e.to_string())?;
        Ok(())
    }
}
