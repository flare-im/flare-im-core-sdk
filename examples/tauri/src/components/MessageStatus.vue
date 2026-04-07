<template>
  <div class="message-status">
    <!-- 发送中状态（status === 1，显示旋转动画） -->
    <div v-if="status === 1" class="status-sending">
      <svg class="status-icon rotating" viewBox="0 0 16 16" fill="none">
        <circle cx="8" cy="8" r="6" stroke="#888888" stroke-width="1.5" stroke-dasharray="3 3"/>
      </svg>
    </div>
    
    <!-- 已发送：单勾灰；已送达：单勾绿（参照图） -->
    <div v-else-if="status === 2" class="status-sent" title="已发送">
      <svg class="status-icon status-one-check" viewBox="0 0 16 16" fill="none" aria-hidden="true">
        <path d="M12 4L6 10L4 8" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round"/>
      </svg>
    </div>
    <div v-else-if="status === 3" class="status-delivered" title="已送达">
      <svg class="status-icon status-one-check" viewBox="0 0 16 16" fill="none" aria-hidden="true">
        <path d="M12 4L6 10L4 8" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round"/>
      </svg>
    </div>
    
    <!-- 已读：两个勾 ✅✅（绿色） -->
    <div v-else-if="status === 4" class="status-read" title="已读">
      <svg class="status-icon status-two-checks" viewBox="0 0 18 16" fill="none" aria-hidden="true">
        <path 
          d="M1.5 8.5L4.5 11.5L8 7" 
          stroke="#4CAF50" 
          stroke-width="1.5" 
          stroke-linecap="round" 
          stroke-linejoin="round"
        />
        <path 
          d="M7 8.5L10 11.5L16 4.5" 
          stroke="#4CAF50" 
          stroke-width="1.5" 
          stroke-linecap="round" 
          stroke-linejoin="round"
        />
      </svg>
    </div>
    
    <!-- 发送失败状态（status === 5） -->
    <div v-else-if="status === 5" class="status-failed">
      <svg class="status-icon" viewBox="0 0 16 16" fill="none">
        <circle cx="8" cy="8" r="6" stroke="#FF6B6B" stroke-width="1.5"/>
        <path d="M8 5V8M8 11H8.01" stroke="#FF6B6B" stroke-width="1.5" stroke-linecap="round"/>
      </svg>
      <span class="status-text">发送失败</span>
    </div>
    
    <!-- 已撤回状态（status === 6） -->
    <div v-else-if="status === 6" class="status-recalled">
      <span class="status-text">已撤回</span>
    </div>
    
    <!-- 其他状态（status === 0 或 undefined）不显示 -->
  </div>
</template>

<script setup lang="ts">
interface Props {
  status: number;
  size?: number;
}

withDefaults(defineProps<Props>(), {
  size: 16
});

</script>

<style scoped>
.message-status {
  display: flex;
  align-items: center;
  gap: 4px;
  font-size: 0;
}

.status-icon {
  width: 18px;
  height: 18px;
  display: block;
}

.status-text {
  font-size: var(--font-size-xs, 12px);
  color: var(--wechat-text-secondary, #888888);
  margin-left: 2px;
}

/* 旋转动画 - 发送中状态 */
@keyframes rotate {
  from {
    transform: rotate(0deg);
  }
  to {
    transform: rotate(360deg);
  }
}

.rotating {
  animation: rotate 1s linear infinite;
}

/* 状态颜色：送达单勾灰，已读双勾绿 */
.status-sending .status-icon {
  opacity: 0.6;
  color: #888888;
}

.status-sent .status-icon {
  color: var(--wechat-status-sent, #888888);
  opacity: 0.9;
}

/* 已送达：绿色单勾（参照图） */
.status-delivered .status-icon {
  color: var(--wechat-status-read, #07C160);
  opacity: 1;
}

.status-read .status-icon {
  color: var(--wechat-status-read, #07C160);
  filter: drop-shadow(0 0 2px rgba(7, 193, 96, 0.3));
}

.status-failed .status-icon {
  filter: drop-shadow(0 0 2px rgba(255, 107, 107, 0.3));
}

/* 悬停效果 */
.message-status:hover .status-icon {
  transform: scale(1.1);
  transition: transform 0.2s ease;
}

.status-failed:hover .status-icon {
  animation: shake 0.5s ease-in-out;
}

@keyframes shake {
  0%, 100% { transform: translateX(0); }
  25% { transform: translateX(-2px); }
  75% { transform: translateX(2px); }
}

/* 深色模式适配 */
@media (prefers-color-scheme: dark) {
  .status-text {
    color: var(--wechat-text-secondary, #999999);
  }
  
  .status-sending .status-icon {
    opacity: 0.8;
  }
  
  .status-sent .status-icon {
    opacity: 1;
  }
}

/* 高DPI屏幕优化 */
@media (-webkit-min-device-pixel-ratio: 2), (min-resolution: 192dpi) {
  .status-icon {
    stroke-width: 1.25;
  }
}

/* 动画性能优化 */
.status-icon {
  will-change: transform;
  backface-visibility: hidden;
  transform: translateZ(0);
}

.rotating {
  will-change: transform;
  transform-origin: center;
}
</style>
