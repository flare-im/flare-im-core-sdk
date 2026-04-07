<script setup lang="ts">
import { onMounted } from "vue";
import { invoke } from "@tauri-apps/api/core";
import { resolveSdkDataUrl } from "./utils/dataUrl";

const isTauriEnv = typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;

/** 与 Login / Chat 一致：`sdk_init` 需 `{ args: { environment, sdkConfig: { dataUrl } } }` */
onMounted(async () => {
  if (!isTauriEnv) {
    console.warn("Tauri environment not detected; skipping sdk_init in browser preview");
    return;
  }
  try {
    const environment = import.meta.env.DEV ? "development" : "production";
    const dataUrl = await resolveSdkDataUrl();
    await invoke("sdk_init", {
      args: { environment, sdkConfig: { dataUrl } },
    });
    console.log("SDK initialized on app startup", dataUrl);
  } catch (e) {
    console.error("Failed to initialize SDK:", e);
  }
});
</script>

<template>
  <router-view />
</template>

<style scoped>
.logo.vite:hover {
  filter: drop-shadow(0 0 2em #747bff);
}

.logo.vue:hover {
  filter: drop-shadow(0 0 2em #249b73);
}

</style>
<style>
/* 微信UI设计规范v3.2 - Design Token系统 */
:root {
  /* 颜色系统 - 严格遵循微信规范 */
  --wechat-primary: #07C160;
  --wechat-bubble-sent: #DCF8C6;      /* 发送方气泡背景色 */
  --wechat-bubble-received: #FFFFFF;   /* 接收方气泡背景色 */
  --wechat-timestamp: #B2B2B2;        /* 时间戳颜色 */
  --wechat-background: #F5F5F5;        /* 聊天背景色 */
  --wechat-divider: #E5E5E5;           /* 分割线颜色 */
  --wechat-text-primary: #000000;      /* 主要文本颜色 */
  --wechat-text-secondary: #888888;    /* 次要文本颜色 */
  --wechat-status-sent: #888888;       /* 已发送状态颜色 */
  --wechat-status-read: #07C160;      /* 已读状态颜色 */
  
  /* 间距系统 - 8px基础单位 */
  --spacing-xs: 4px;
  --spacing-sm: 8px;
  --spacing-md: 12px;   /* 消息气泡左右间距 */
  --spacing-lg: 16px;
  --spacing-xl: 24px;
  
  /* 圆角系统 */
  --radius-sm: 4px;
  --radius-md: 8px;     /* 消息气泡圆角 */
  --radius-lg: 12px;
  --radius-round: 50%;  /* 头像圆形 */
  
  /* 字体系统 */
  --font-size-xs: 12px;  /* 时间戳字体 */
  --font-size-sm: 14px;  /* 消息内容字体 */
  --font-size-md: 16px;
  --line-height: 1.4;
  
  /* 阴影系统 */
  --shadow-bubble: 0 1px 2px rgba(0,0,0,0.1);  /* 消息气泡阴影 */
  --shadow-card: 0 2px 8px rgba(0,0,0,0.1);
  
  /* 尺寸系统 */
  --avatar-size: 40px;   /* 头像尺寸 */
  --bubble-max-width: 78%; /* 消息气泡最大宽度（相对聊天内容区宽度） */
  --input-height: 40px;   /* 输入框高度 */
}

/* 深色模式适配 */
@media (prefers-color-scheme: dark) {
  :root {
    --wechat-bubble-sent: #2C3E50;
    --wechat-bubble-received: #1A1A1A;
    --wechat-background: #0A0A0A;
    --wechat-divider: #2C2C2C;
    --wechat-text-primary: #FFFFFF;
    --wechat-text-secondary: #999999;
  }
}

/* 移动端适配 */
@media (max-width: 768px) {
  :root {
    --avatar-size: 36px;
    --bubble-max-width: 78%;
  }
}

/* 全局基础样式重置 */
* {
  box-sizing: border-box;
}

/* 消息列表滚动条样式 */
.messages::-webkit-scrollbar {
  width: 6px;
}

.messages::-webkit-scrollbar-track {
  background: transparent;
}

.messages::-webkit-scrollbar-thumb {
  background: #CCCCCC;
  border-radius: 3px;
}

.messages::-webkit-scrollbar-thumb:hover {
  background: #AAAAAA;
}
:root {
  --bubble-bg: #fff;
  --bubble-bg-self: #e6f4ff;
  --bubble-border: #eee;
  --sidebar-hover: #f7f8fa;
  --sidebar-active: #e6f4ff;
  --bg: #1f2023;
  --panel-bg: #26282c;
  --panel-border: #3a3d42;
  --text: #e6e8ea;
  --muted-text: #a9adb3;
}

html, body {
  margin: 0;
  padding: 0;
  height: 100%;
  width: 100%;
  overflow: hidden; /* 防止 body 滚动条 */
}

#app {
  height: 100%;
  width: 100%;
  overflow: hidden; /* 防止 app 滚动条 */
  background: var(--bg);
  color: var(--text);
}
.sidebar { background: var(--panel-bg); color: var(--text); border-right: 1px solid var(--panel-border) !important; }
.messages { background: #fff !important; color: #222; }
.a-typography, .a-typography-text { color: var(--text); }
</style>
