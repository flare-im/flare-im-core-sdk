<template>
  <div class="avatar-container" :style="{ width: size + 'px', height: size + 'px' }">
    <img
      v-if="avatarUrl && !hasError"
      :src="avatarUrl"
      :alt="displayName"
      class="avatar-image"
      :class="{ 'avatar-gif': isGif }"
      @error="onImageError"
      @load="onImageLoad"
    />
    <div v-else class="avatar-fallback" :style="{ backgroundColor: fallbackColor }">
      <span class="avatar-text">{{ initials }}</span>
    </div>
    <div v-if="showStatus" class="avatar-status" :class="statusClass"></div>
  </div>
</template>

<script setup lang="ts">
import { ref, computed, watch } from 'vue';

interface Props {
  userId: string;
  displayName?: string;
  avatarUrl?: string;
  size?: number;
  showStatus?: boolean;
  status?: 'online' | 'offline' | 'busy';
}

const props = withDefaults(defineProps<Props>(), {
  displayName: '',
  avatarUrl: '',
  size: 40,
  showStatus: false,
  status: 'offline'
});

const hasError = ref(false);
const imageSize = ref(0);

// 计算首字母
const initials = computed(() => {
  const name = props.displayName || props.userId;
  if (!name) return 'U';
  return name.charAt(0).toUpperCase();
});

// 生成基于用户ID的颜色
const fallbackColor = computed(() => {
  const colors = [
    '#FF6B6B', '#4ECDC4', '#45B7D1', '#96CEB4', '#FFEAA7',
    '#DDA0DD', '#98D8C8', '#F7DC6F', '#BB8FCE', '#85C1E9'
  ];
  let hash = 0;
  const uid = props.userId || '';
  for (let i = 0; i < uid.length; i++) {
    hash = uid.charCodeAt(i) + ((hash << 5) - hash);
  }
  return colors[Math.abs(hash) % colors.length];
});

// 检查是否为GIF
const isGif = computed(() => {
  return props.avatarUrl?.toLowerCase().endsWith('.gif') || false;
});

// 状态样式
const statusClass = computed(() => {
  return `avatar-status-${props.status}`;
});

// 图片加载错误处理
const onImageError = () => {
  hasError.value = true;
};

// 图片加载成功处理
const onImageLoad = (event: Event) => {
  const img = event.target as HTMLImageElement;
  imageSize.value = img.naturalWidth * img.naturalHeight;
  
  // GIF大小检查（限制2MB）
  if (isGif.value && imageSize.value > 0) {
    const estimatedSize = imageSize.value * 4; // 估算文件大小
    if (estimatedSize > 2 * 1024 * 1024) { // 超过2MB
      hasError.value = true;
    }
  }
};

// 监听avatarUrl变化
watch(() => props.avatarUrl, () => {
  hasError.value = false;
});
</script>

<style scoped>
.avatar-container {
  position: relative;
  border-radius: 50%;
  overflow: hidden;
  flex-shrink: 0;
  background-color: transparent;
}

.avatar-image {
  width: 100%;
  height: 100%;
  object-fit: cover;
  display: block;
  border: none;
  border-radius: 50%;
  background: transparent;
}

.avatar-gif {
  /* GIF动画优化 */
  image-rendering: -webkit-optimize-contrast;
  image-rendering: crisp-edges;
}

.avatar-fallback {
  width: 100%;
  height: 100%;
  display: flex;
  align-items: center;
  justify-content: center;
  border-radius: 50%;
  transition: background-color 0.2s ease;
  border: none;
  /* 背景色将通过内联样式设置 */
}

.avatar-text {
  color: #FFFFFF;
  font-weight: 500;
  font-size: calc(var(--avatar-size, 40px) * 0.4);
  line-height: 1;
  user-select: none;
}

.avatar-status {
  position: absolute;
  bottom: 2px;
  right: 2px;
  width: 8px;
  height: 8px;
  border-radius: 50%;
  border: 2px solid #FFFFFF;
  box-shadow: 0 1px 2px rgba(0,0,0,0.1);
}

.avatar-status-online {
  background-color: #07C160;
}

.avatar-status-offline {
  background-color: #C0C0C0;
}

.avatar-status-busy {
  background-color: #FF6B6B;
}

/* Material Design风格悬停效果 */
.avatar-container:hover {
  box-shadow: 0 2px 8px rgba(0,0,0,0.15);
  transform: scale(1.05);
  transition: all 0.2s ease;
}

/* 高DPI屏幕适配 */
@media (-webkit-min-device-pixel-ratio: 2), (min-resolution: 192dpi) {
  .avatar-container {
    /* 移除高DPI屏幕上的边框，避免黑框 */
    border: none;
  }
}

/* 深色模式适配 */
@media (prefers-color-scheme: dark) {
  .avatar-text {
    color: #FFFFFF;
  }
  
  .avatar-status {
    border-color: #1A1A1A;
  }
}
</style>