<template>
  <div v-if="shouldShowTimestamp" class="timestamp-container">
    <span class="timestamp-text">{{ formattedTime }}</span>
  </div>
</template>

<script setup lang="ts">
import { computed } from 'vue';

interface Props {
  /** 毫秒时间戳（number）或 ISO 8601 字符串 */
  timestamp: string | number;
  previousTimestamp?: string | number;
  format?: 'time' | 'date' | 'datetime';
}

const props = withDefaults(defineProps<Props>(), {
  previousTimestamp: undefined,
  format: 'time'
});

function toMs(v: string | number | undefined): number {
  if (v === undefined) return 0;
  const ms = typeof v === 'number' ? v : new Date(v).getTime();
  return Number.isNaN(ms) ? 0 : ms;
}

// 有效时间戳至少为 2020-01-01 附近，避免 0 或错误值显示为 1970
const VALID_TS_MIN = new Date('2020-01-01').getTime();
function isValidTimestamp(ms: number): boolean {
  return ms >= VALID_TS_MIN && ms <= Date.now() + 86400000 * 365;
}

// 5分钟间隔阈值（毫秒）
const TIME_THRESHOLD = 5 * 60 * 1000;

// 是否应该显示时间戳
const shouldShowTimestamp = computed(() => {
  if (props.previousTimestamp === undefined) return true;
  const currentTime = toMs(props.timestamp);
  const previousTime = toMs(props.previousTimestamp);
  if (!isValidTimestamp(currentTime)) return true;
  return Math.abs(currentTime - previousTime) > TIME_THRESHOLD;
});

// 格式化时间显示
const formattedTime = computed(() => {
  const ms = toMs(props.timestamp);
  if (!ms || !isValidTimestamp(ms)) return '刚刚';
  const date = new Date(ms);
  
  // 如果是今天，只显示时间
  if (isToday(date)) {
    return formatTime(date);
  }
  
  // 如果是昨天，显示"昨天"
  if (isYesterday(date)) {
    return '昨天 ' + formatTime(date);
  }
  
  // 如果是本周，显示星期
  if (isThisWeek(date)) {
    return getWeekDay(date) + ' ' + formatTime(date);
  }
  
  // 否则显示完整日期
  return formatDateTime(date);
});

// 判断是否为今天
function isToday(date: Date): boolean {
  const today = new Date();
  return date.getDate() === today.getDate() &&
         date.getMonth() === today.getMonth() &&
         date.getFullYear() === today.getFullYear();
}

// 判断是否为昨天
function isYesterday(date: Date): boolean {
  const yesterday = new Date();
  yesterday.setDate(yesterday.getDate() - 1);
  return date.getDate() === yesterday.getDate() &&
         date.getMonth() === yesterday.getMonth() &&
         date.getFullYear() === yesterday.getFullYear();
}

// 判断是否为本周
function isThisWeek(date: Date): boolean {
  const now = new Date();
  const weekStart = new Date(now);
  weekStart.setDate(now.getDate() - now.getDay());
  weekStart.setHours(0, 0, 0, 0);
  
  return date >= weekStart;
}

// 获取星期几
function getWeekDay(date: Date): string {
  const days = ['周日', '周一', '周二', '周三', '周四', '周五', '周六'];
  return days[date.getDay()];
}

// 格式化时间 (HH:mm)
function formatTime(date: Date): string {
  const hours = date.getHours().toString().padStart(2, '0');
  const minutes = date.getMinutes().toString().padStart(2, '0');
  return `${hours}:${minutes}`;
}

// 格式化日期时间
function formatDateTime(date: Date): string {
  const year = date.getFullYear();
  const month = (date.getMonth() + 1).toString().padStart(2, '0');
  const day = date.getDate().toString().padStart(2, '0');
  const time = formatTime(date);
  
  // 如果是今年，不显示年份
  const now = new Date();
  if (date.getFullYear() === now.getFullYear()) {
    return `${month}月${day}日 ${time}`;
  }
  
  return `${year}年${month}月${day}日 ${time}`;
}
</script>

<style scoped>
.timestamp-container {
  display: flex;
  justify-content: center;
  margin: 16px 0;
  padding: 0 16px;
}

.timestamp-text {
  font-size: var(--font-size-xs, 12px);
  line-height: var(--line-height, 1.4);
  color: var(--wechat-timestamp, #B2B2B2);
  background-color: rgba(0, 0, 0, 0.05);
  padding: 4px 8px;
  border-radius: var(--radius-sm, 4px);
  user-select: none;
  white-space: nowrap;
}

/* 深色模式适配 */
@media (prefers-color-scheme: dark) {
  .timestamp-text {
    background-color: rgba(255, 255, 255, 0.1);
  }
}

/* 动画效果 */
.timestamp-container {
  animation: fadeIn 0.3s ease-out;
}

@keyframes fadeIn {
  from {
    opacity: 0;
    transform: translateY(-10px);
  }
  to {
    opacity: 1;
    transform: translateY(0);
  }
}
</style>
