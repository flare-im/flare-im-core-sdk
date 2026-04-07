<template>
  <div class="feishu-content-view">
    <component
      :is="viewComponent"
      v-if="viewComponent && content"
      v-bind="viewExtraProps"
    />
    <div v-else-if="content" class="feishu-content-fallback">
      {{ fallbackText }}
    </div>
  </div>
</template>

<script setup lang="ts">
import { computed, defineAsyncComponent } from 'vue';
import type { ContentElem } from '../../types';
import { getContentDecodedPreview } from '../../utils/message';

const TextView = defineAsyncComponent(() => import('./views/TextView.vue'));
const ImageView = defineAsyncComponent(() => import('./views/ImageView.vue'));
const VideoView = defineAsyncComponent(() => import('./views/VideoView.vue'));
const AudioView = defineAsyncComponent(() => import('./views/AudioView.vue'));
const FileView = defineAsyncComponent(() => import('./views/FileView.vue'));
const LocationView = defineAsyncComponent(() => import('./views/LocationView.vue'));
const CardView = defineAsyncComponent(() => import('./views/CardView.vue'));
const StickerView = defineAsyncComponent(() => import('./views/StickerView.vue'));
const EmojiView = defineAsyncComponent(() => import('./views/EmojiView.vue'));
const GifView = defineAsyncComponent(() => import('./views/GifView.vue'));
const QuoteView = defineAsyncComponent(() => import('./views/QuoteView.vue'));
const LinkCardView = defineAsyncComponent(() => import('./views/LinkCardView.vue'));
const ForwardView = defineAsyncComponent(() => import('./views/ForwardView.vue'));
const ThreadView = defineAsyncComponent(() => import('./views/ThreadView.vue'));
const MiniProgramView = defineAsyncComponent(() => import('./views/MiniProgramView.vue'));
const RichTextView = defineAsyncComponent(() => import('./views/RichTextView.vue'));
const MarkdownView = defineAsyncComponent(() => import('./views/MarkdownView.vue'));
const ImageGroupView = defineAsyncComponent(() => import('./views/ImageGroupView.vue'));
const SystemView = defineAsyncComponent(() => import('./views/SystemView.vue'));
const NotificationView = defineAsyncComponent(() => import('./views/NotificationView.vue'));
const VoteView = defineAsyncComponent(() => import('./views/VoteView.vue'));
const TaskView = defineAsyncComponent(() => import('./views/TaskView.vue'));
const ScheduleView = defineAsyncComponent(() => import('./views/ScheduleView.vue'));
const AnnouncementView = defineAsyncComponent(() => import('./views/AnnouncementView.vue'));
const CustomView = defineAsyncComponent(() => import('./views/CustomView.vue'));
const PlaceholderView = defineAsyncComponent(() => import('./views/PlaceholderView.vue'));

const componentMap: Record<string, ReturnType<typeof defineAsyncComponent>> = {
  text: TextView,
  image: ImageView,
  video: VideoView,
  audio: AudioView,
  file: FileView,
  location: LocationView,
  card: CardView,
  sticker: StickerView,
  emoji: EmojiView,
  gif: GifView,
  quote: QuoteView,
  linkCard: LinkCardView,
  forward: ForwardView,
  thread: ThreadView,
  miniProgram: MiniProgramView,
  richText: RichTextView,
  markdown: MarkdownView,
  imageGroup: ImageGroupView,
  system: SystemView,
  notification: NotificationView,
  vote: VoteView,
  task: TaskView,
  schedule: ScheduleView,
  announcement: AnnouncementView,
  custom: CustomView,
  placeholder: PlaceholderView,
};

interface Props {
  content: ContentElem | null | undefined;
  isSelf?: boolean;
  /** 用于语音未读红点等（仅 audio 视图使用） */
  messageId?: string;
}

const props = withDefaults(defineProps<Props>(), {
  isSelf: false,
  messageId: '',
});

const viewComponent = computed(() => {
  const type = props.content?.contentType;
  if (!type) return null;
  return componentMap[type] ?? null;
});

const viewExtraProps = computed(() => {
  const base: Record<string, unknown> = {
    content: props.content,
    isSelf: props.isSelf,
  };
  if (props.content?.contentType === 'audio' && props.messageId) {
    base.messageId = props.messageId;
  }
  return base;
});

const fallbackText = computed(() => {
  if (!props.content) return '';
  return getContentDecodedPreview(props.content) || '[未知消息类型]';
});
</script>

<style scoped>
.feishu-content-view {
  --feishu-primary: #3370ff;
  --feishu-primary-hover: #2b5dd9;
  --feishu-bg-card: #f7f8fa;
  --feishu-bg-hover: #f2f3f5;
  --feishu-border: #e5e6eb;
  --feishu-text-primary: #1d2129;
  --feishu-text-secondary: #86909c;
  --feishu-text-tertiary: #c9cdd4;
  --feishu-radius: 8px;
  --feishu-radius-sm: 4px;
  min-width: 0;
}

.feishu-content-fallback {
  font-size: 13px;
  color: var(--feishu-text-secondary);
  padding: 4px 0;
}
</style>
