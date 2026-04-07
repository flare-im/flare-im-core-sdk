<template>
  <div class="feishu-schedule">
    <div class="feishu-schedule-card">
      <span class="feishu-schedule-icon">📅</span>
      <div class="feishu-schedule-info">
        <span class="feishu-schedule-title">{{ payload.title || '日程' }}</span>
        <span class="feishu-schedule-time">{{ timeRange }}</span>
      </div>
    </div>
  </div>
</template>

<script setup lang="ts">
import { computed } from 'vue';
import type { ContentElem } from '../../../types';

interface Props {
  content: ContentElem;
  isSelf: boolean;
}

const props = defineProps<Props>();

const payload = computed(() => {
  if (props.content.contentType !== 'schedule' || !props.content.schedule) {
    return { scheduleId: '', title: '', startTime: 0, endTime: 0 };
  }
  return props.content.schedule;
});

const timeRange = computed(() => {
  const p = payload.value;
  if (!p.startTime && !p.endTime) return '';
  const fmt = (ms: number) => {
    const d = new Date(ms);
    return `${d.getMonth() + 1}/${d.getDate()} ${d.getHours().toString().padStart(2, '0')}:${d.getMinutes().toString().padStart(2, '0')}`;
  };
  if (p.startTime && p.endTime) return `${fmt(p.startTime)} - ${fmt(p.endTime)}`;
  if (p.startTime) return fmt(p.startTime);
  return fmt(p.endTime!);
});
</script>

<style scoped>
.feishu-schedule {
  max-width: 280px;
}

.feishu-schedule-card {
  display: flex;
  align-items: center;
  gap: 10px;
  padding: 10px 14px;
  background: var(--feishu-bg-card, #f7f8fa);
  border: 1px solid var(--feishu-border, #e5e6eb);
  border-radius: var(--feishu-radius, 8px);
}

.feishu-schedule-icon {
  font-size: 22px;
  flex-shrink: 0;
}

.feishu-schedule-info {
  flex: 1;
  min-width: 0;
  display: flex;
  flex-direction: column;
  gap: 2px;
}

.feishu-schedule-title {
  font-size: 14px;
  color: var(--feishu-text-primary, #1d2129);
}

.feishu-schedule-time {
  font-size: 12px;
  color: var(--feishu-text-secondary, #86909c);
}
</style>
