use flare_proto::{
    AudioContent, FileContent, ImageContent, Message, MessageContent, TextContent, VideoContent,
};
use prost::Message as ProstMessage;

pub struct MessageBuilder {
    message: Message,
}

impl MessageBuilder {
    pub fn new() -> Self {
        Self {
            message: Message::default(),
        }
    }

    pub fn id(mut self, id: String) -> Self {
        self.message.id = id;
        self
    }

    pub fn session_id(mut self, session_id: String) -> Self {
        self.message.session_id = session_id;
        self
    }

    pub fn sender_id(mut self, sender_id: String) -> Self {
        self.message.sender_id = sender_id;
        self
    }

    pub fn priority(mut self, priority: i32) -> Self {
        self.message
            .extra
            .insert("priority".to_string(), priority.to_string());
        self
    }

    pub fn text(mut self, text: String) -> Self {
        let content = TextContent {
            text,
            mentions: vec![],
        };
        self.message.content = Some(MessageContent {
            content: Some(flare_proto::flare::common::v1::message_content::Content::Text(content)),
            extensions: vec![],
        });
        self
    }

    pub fn image(mut self, image: ImageContent) -> Self {
        self.message.content = Some(MessageContent {
            content: Some(flare_proto::flare::common::v1::message_content::Content::Image(image)),
            extensions: vec![],
        });
        self
    }

    pub fn video(mut self, video: VideoContent) -> Self {
        self.message.content = Some(MessageContent {
            content: Some(flare_proto::flare::common::v1::message_content::Content::Video(video)),
            extensions: vec![],
        });
        self
    }

    pub fn audio(mut self, audio: AudioContent) -> Self {
        self.message.content = Some(MessageContent {
            content: Some(flare_proto::flare::common::v1::message_content::Content::Audio(audio)),
            extensions: vec![],
        });
        self
    }

    pub fn file(mut self, file: FileContent) -> Self {
        self.message.content = Some(MessageContent {
            content: Some(flare_proto::flare::common::v1::message_content::Content::File(file)),
            extensions: vec![],
        });
        self
    }

    pub fn metadata(mut self, key: String, value: String) -> Self {
        self.message.extra.insert(key, value);
        self
    }

    pub fn build(self) -> Message {
        self.message
    }

    pub fn encode(self) -> anyhow::Result<Vec<u8>> {
        let mut buf = Vec::new();
        self.message
            .encode(&mut buf)
            .map_err(|e| anyhow::anyhow!("{}", e))?;
        Ok(buf)
    }
}
