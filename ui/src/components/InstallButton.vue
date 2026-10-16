<script setup>
// 机身上唯一那个键。三种状态：待命（橙、可按）/ 进行中（灰、锁住）/ 完成（灰、可按 = 重新检查）
const props = defineProps({
  state: { type: String, default: "idle" }, // idle | busy | done
  /// 有东西要重启才生效：完成之后这一个键变成两个（立刻重启 / 稍后重启）
  reboot: { type: Boolean, default: false },
});

defineEmits(["press", "restart"]);

const LABEL = {
  idle: "安装",
  busy: "安装中",
  done: "完成",
  fail: "重试",
};
</script>

<template>
  <div v-if="state === 'done' && reboot" class="reboot-keys">
    <button class="install" @click="$emit('restart')">立刻重启</button>
    <button class="install ghost" @click="$emit('press')">稍后重启</button>
  </div>
  <button v-else class="install" :disabled="state === 'busy'" @click="$emit('press')">
    {{ LABEL[state] ?? LABEL.idle }}
  </button>
</template>
