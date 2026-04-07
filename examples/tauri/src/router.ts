import { createRouter, createWebHistory } from "vue-router";
import Login from "./views/Login.vue";
import Chat from "./views/Chat.vue";

export const router = createRouter({
  history: createWebHistory(),
  routes: [
    { path: "/", component: Login },
    { path: "/chat", component: Chat },
  ],
});
