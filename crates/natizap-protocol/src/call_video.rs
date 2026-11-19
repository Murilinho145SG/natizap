//! A call's pictures: the camera's H.264 going out, the other end's coming
//! in, and turning a voice call into a video call, either way.

use gpui::{App, AppContext as _, Task};
use whatsapp_rust::voip::{VideoFrame, VideoUpgradeToken};

use crate::Protocol;
use crate::actions::ActionError;
use crate::live_call::LiveCall;

/// Pictures the other end sent waiting to be decoded before new ones are
/// dropped.
const SCREEN_BACKLOG: usize = 8;

/// A call's video.
#[derive(Debug, Clone)]
pub struct Video {
    /// The camera's pictures to send: H.264 access units in Annex-B,
    /// fifteen a second. A camera that is off simply sends none.
    pub camera: async_channel::Receiver<Vec<u8>>,
    /// Where the other end's pictures go: each access unit, with how many
    /// quarter turns clockwise their phone was held.
    pub screen: async_channel::Sender<(Vec<u8>, u8)>,
}

impl Video {
    /// The ends the call library takes: the camera as it is, and a screen
    /// fed from the library's frames by a task that ends with the call.
    pub(crate) fn endpoints(
        self,
        cx: &App,
    ) -> (
        async_channel::Receiver<Vec<u8>>,
        async_channel::Sender<VideoFrame>,
    ) {
        let (frames, received) = async_channel::bounded::<VideoFrame>(SCREEN_BACKLOG);
        let screen = self.screen;
        cx.background_spawn(async move {
            while let Ok(frame) = received.recv().await {
                // Full means the decoder is behind: this picture is lost.
                if let Err(async_channel::TrySendError::Closed(_)) =
                    screen.try_send((frame.data, frame.orientation))
                {
                    return;
                }
            }
        })
        .detach();
        (self.camera, frames)
    }
}

/// The other end asking for video, to answer with
/// [`Protocol::accept_video`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct VideoRequest(pub(crate) VideoUpgradeToken);

impl Protocol {
    /// Turns `call` into a video call with `video`, asking the other end
    /// to turn their camera on too.
    pub fn start_video(
        &self,
        call: &LiveCall,
        video: Video,
        cx: &App,
    ) -> Task<Result<(), ActionError>> {
        let handle = call.handle.clone();
        let (camera, screen) = video.endpoints(cx);
        self.connection.run(cx, move |_| async move {
            handle
                .start_video(camera, screen)
                .await
                .map_err(|error| ActionError::refused(&error))
        })
    }

    /// Takes up the other end's `request` for video on `call`, with
    /// `video`.
    pub fn accept_video(
        &self,
        call: &LiveCall,
        request: VideoRequest,
        video: Video,
        cx: &App,
    ) -> Task<Result<(), ActionError>> {
        let handle = call.handle.clone();
        let (camera, screen) = video.endpoints(cx);
        self.connection.run(cx, move |_| async move {
            handle
                .accept_video(request.0, camera, screen)
                .await
                .map_err(|error| ActionError::refused(&error))
        })
    }
}
