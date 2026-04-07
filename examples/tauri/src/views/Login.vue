<script setup lang="ts">
import { ref, onMounted } from "vue";
import { useRouter } from "vue-router";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { IconUser } from "@arco-design/web-vue/es/icon";
import { useImEvents } from "../composables/useImEvents";
import { resolveSdkDataUrl } from "../utils/dataUrl";

const router = useRouter();
const userId = ref("");
const loading = ref(false);
const error = ref("");
const logs = ref<string[]>([]);
/** 连接成功后的阶段：connecting -> syncing -> done */
const phase = ref<"idle" | "connecting" | "syncing" | "done">("idle");

const { syncProgress, waitForInitSync, waitForFullSync, listenersReady, clearSyncState } = useImEvents({});

onMounted(async () => {
  try {
    await listen<{ user_id: string }>("sdk_auto_login", (event) => {
      const savedUserId = event.payload.user_id;
      log(`检测到保存的登录状态，用户 ID: ${savedUserId}`);
      userId.value = savedUserId;
      login();
    });
  } catch (e) {
    console.error("Failed to listen to sdk_auto_login event:", e);
  }
});

function log(s: string) {
  const line = `[${new Date().toLocaleTimeString()}] ${s}`;
  logs.value.push(line);
  console.log(line);
}

function formatError(e: unknown): string {
  if (e && typeof e === "object") {
    if ("message" in e && typeof (e as { message: string }).message === "string") {
      return (e as { message: string }).message;
    }
    try {
      return JSON.stringify(e);
    } catch {
      return String(e);
    }
  }
  return String(e);
}

async function login() {
  const uid = userId.value.trim();
  if (!uid) {
    error.value = "请输入有效的用户 ID";
    return;
  }
  if (loading.value) return;
  loading.value = true;
  error.value = "";
  phase.value = "connecting";
  clearSyncState();
  let connected = false;
  try {
    const isTauriEnv = typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;
    if (!isTauriEnv) {
      error.value = "当前为浏览器预览环境，无法调用 Tauri 后端，请使用 Tauri 应用窗口运行";
      return;
    }
    log(`开始登录，用户: ${uid}`);

    const environment = "development";
    const dataUrl = await resolveSdkDataUrl();
    try {
      await invoke("sdk_init", {
        args: {
          environment,
          sdkConfig: { dataUrl },
        },
      });
      log(`SDK 初始化成功 dataUrl=${dataUrl}`);
    } catch (initErr: unknown) {
      log(`SDK 初始化失败: ${formatError(initErr)}`);
      throw initErr;
    }

    let token = "";
    try {
      token = await invoke<string>("sdk_generate_test_token", {
        secret: "insecure-secret",
        issuer: "flare-im-core",
        userId: uid,
      });
    } catch (tokenErr: unknown) {
      const tmsg = formatError(tokenErr);
      log(`生成 Token 失败: ${tmsg}`);
      throw tokenErr;
    }
    await listenersReady;
    log("调用 sdk_login");
    await invoke("sdk_login", { userId: uid, token });
    log("连接成功，正在同步数据...");
    phase.value = "syncing";

    await waitForInitSync(20000);
    phase.value = "done";
    log("初始化同步完成（Init），进入消息页面");
    void waitForFullSync(120000)
      .then(() => {
        log("后台全量同步完成（Background）");
      })
      .catch((syncErr: unknown) => {
        log(`后台同步未完成: ${formatError(syncErr)}`);
      });
    connected = true;
    localStorage.setItem("userId", uid);
    router.push("/chat");
  } catch (e: unknown) {
    const msg = formatError(e);
    error.value = msg;
    log(`连接失败: ${msg}`);
    if (msg.includes("Connection refused") || msg.includes("not accepting connections")) {
      error.value = "无法连接到服务器，请检查服务器是否运行";
      log("💡 提示：服务器可能未运行，请先启动服务器");
    } else if (msg.includes("timeout") || msg.includes("timed out")) {
      error.value = "连接超时，请检查网络连接";
    } else if (msg.includes("等待同步超时")) {
      error.value = "数据同步超时，请检查网络后重试";
    } else if (msg.includes("DNS") || msg.includes("resolve") || msg.includes("No such host")) {
      error.value = "无法解析服务器地址，请检查服务器 URL 配置";
    } else if (msg.includes("authentication") || msg.includes("认证") || msg.includes("token")) {
      error.value = "认证失败，请检查 Token 配置";
    } else if (msg.includes("Connection lost") || msg.includes("连接丢失")) {
      error.value = "连接已丢失，请重试";
    }
  } finally {
    if (!connected) {
      log("连接失败，仍停留在登录页");
    }
    loading.value = false;
    phase.value = "idle";
  }
}
</script>

<template>
  <div class="login-container">
    <div class="login-card">
      <div class="login-header">
        <div class="logo">💬</div>
        <a-typography-title :heading="3" class="login-title">Flare IM</a-typography-title>
        <a-typography-text class="login-subtitle">即时通讯客户端</a-typography-text>
      </div>
      
      <div class="login-form">
        <a-form layout="vertical" :model="{ userId }">
          <a-form-item label="用户 ID" :feedback="!!error" :validate-status="error ? 'error' : undefined" :help="error || undefined">
            <a-input 
              v-model="userId" 
              placeholder="请输入用户 ID（例如：123456）" 
              allow-clear 
              size="large"
              autofocus 
              @press-enter="login"
            >
              <template #prefix>
                <icon-user />
              </template>
            </a-input>
          </a-form-item>
          
          <a-form-item>
            <a-button 
              type="primary" 
              size="large" 
              :loading="phase === 'connecting'" 
              :disabled="!userId.trim() || loading"
              long
              @click="login"
            >
              {{ phase === 'connecting' ? '连接中...' : phase === 'syncing' ? '正在同步...' : loading ? '请稍候...' : '登录' }}
            </a-button>
          </a-form-item>
          <div v-if="phase === 'syncing'" class="sync-hint">
            <a-spin size="small" />
            <span>{{ syncProgress ? `同步中 ${Math.round((syncProgress.progress ?? 0) * 100)}%` : "正在加载会话列表..." }}</span>
          </div>
        </a-form>
      </div>
      
      <div v-if="error" class="error-tip">
        <a-alert type="error" :message="error" show-icon />
      </div>
      
      <div v-if="logs.length > 0" class="logs-section">
        <a-collapse>
          <a-collapse-item header="诊断日志" name="logs">
            <pre class="log-lines">{{ logs.join('\n') }}</pre>
          </a-collapse-item>
        </a-collapse>
      </div>
    </div>
  </div>
</template>

<style scoped>
.login-container {
  display: flex;
  align-items: center;
  justify-content: center;
  min-height: 100vh;
  background: linear-gradient(135deg, #667eea 0%, #764ba2 100%);
  padding: 20px;
}

.login-card {
  width: 100%;
  max-width: 420px;
  background: #ffffff;
  border-radius: 16px;
  box-shadow: 0 8px 32px rgba(0, 0, 0, 0.1);
  padding: 40px;
  animation: slideUp 0.3s ease-out;
}

@keyframes slideUp {
  from {
    opacity: 0;
    transform: translateY(20px);
  }
  to {
    opacity: 1;
    transform: translateY(0);
  }
}

.login-header {
  text-align: center;
  margin-bottom: 32px;
}

.logo {
  font-size: 64px;
  margin-bottom: 16px;
  animation: bounce 2s infinite;
}

@keyframes bounce {
  0%, 100% {
    transform: translateY(0);
  }
  50% {
    transform: translateY(-10px);
  }
}

.login-title {
  margin: 0 0 8px 0 !important;
  color: #1d2129;
  font-weight: 600;
}

.login-subtitle {
  color: #86909c;
  font-size: 14px;
}

.login-form {
  margin-bottom: 24px;
}

.error-tip {
  margin-bottom: 16px;
}

.sync-hint {
  display: flex;
  align-items: center;
  gap: 8px;
  margin-top: 12px;
  font-size: 13px;
  color: #86909c;
}

.logs-section {
  margin-top: 16px;
}

.log-lines {
  font-size: 12px;
  line-height: 1.6;
  color: #4e5969;
  background: #f7f8fa;
  padding: 12px;
  border-radius: 8px;
  max-height: 300px;
  overflow-y: auto;
  white-space: pre-wrap;
  word-break: break-all;
}

/* 深色模式适配 */
@media (prefers-color-scheme: dark) {
  .login-card {
    background: #1d2129;
  }
  
  .login-title {
    color: #ffffff !important;
  }
  
  .login-subtitle {
    color: #86909c;
  }
  
  .log-lines {
    background: #2b2b2b;
    color: #a9adb3;
  }
}

/* 移动端适配 */
@media (max-width: 768px) {
  .login-container {
    padding: 16px;
  }
  
  .login-card {
    padding: 24px;
  }
  
  .logo {
    font-size: 48px;
  }
}
</style>
