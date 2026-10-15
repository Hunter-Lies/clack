<script setup>
import { computed, nextTick, onMounted, ref, watch } from "vue";
import ScreenPanel from "./ScreenPanel.vue";
import InstallButton from "./InstallButton.vue";
import FinishMark from "./FinishMark.vue";
import { openExternal } from "../lib/link";

const AUTHOR = "HunterLies";
const AUTHOR_URL = "https://space.bilibili.com/488494586";

const props = defineProps({
  items: { type: Array, required: true },
  state: { type: String, default: "idle" }, // idle | busy | done | fail
  percent: { type: Number, default: 0 },
  failed: { type: Number, default: 0 },
  /// 最后一条失败原因（"msiexec 返回 1603" 这种），出问题时直接摆在状态行上
  error: { type: String, default: "" },
});

defineEmits(["press"]);

const STATUS = {
  idle: "就绪",
  busy: "正在安装",
  done: "安装完成",
  fail: "没装上",
};

const status = computed(() => {
  if (props.state === "fail" && props.failed) {
    const base = `${props.failed} 项没装上`;
    return props.error ? `${base} · ${props.error}` : base;
  }
  return STATUS[props.state] ?? STATUS.idle;
});

// 底部渐隐只在真的装不下时出现：列表短的时候挂着它，等于把最后一行啃掉一半
const rowsEl = ref(null);
const scrollable = ref(false);

async function checkFit() {
  await nextTick();
  const el = rowsEl.value;
  scrollable.value = !!el && el.scrollHeight > el.clientHeight + 1;
}

onMounted(checkFit);
watch(() => props.items.length, checkFit);
watch(() => props.state, checkFit);
</script>

<template>
  <div class="page">
    <ScreenPanel>
      <!-- 装完就把清单和状态行全撤掉，屏幕上只留一个圈打勾 -->
      <FinishMark v-if="state === 'done'" />
      <template v-else>
        <div ref="rowsEl" class="rows" :class="{ scrollable }">
          <div
            v-for="it in items"
            :key="it.id ?? it.nm"
            class="row"
            :class="{ run: it.running, done: it.installed }"
          >
            <i class="dot"></i>
            <span class="nm">{{ it.nm }}</span>
            <span class="ver">{{ it.ver }}</span>
          </div>
        </div>

        <div class="status">
          <span>{{ status }}</span>
          <span class="bar"><i :style="{ width: percent + '%' }"></i></span>
          <span class="pct">{{ Math.round(percent) }}%</span>
        </div>
      </template>
    </ScreenPanel>

    <!-- 屏幕和按键之间这条带子是真机上印厂标的位置（Game Boy / Game Gear / PSP 都在这一条），
         左边放作者，右边是跳转按钮 -->
    <div class="print">
      <span class="by">作者 <b>{{ AUTHOR }}</b></span>
      <button
        class="jump"
        :title="AUTHOR_URL"
        @click="openExternal(AUTHOR_URL)"
      >
        <svg width="12" height="12" viewBox="0 0 12 12" aria-hidden="true">
          <path
            d="M4.7 2.3H2.5A1.3 1.3 0 0 0 1.2 3.6v5.9a1.3 1.3 0 0 0 1.3 1.3h5.9a1.3 1.3 0 0 0 1.3-1.3V7.3"
            stroke="currentColor"
            stroke-width="1.3"
            stroke-linecap="round"
          />
          <path
            d="M7.3 1.1h3.6v3.6M10.7 1.3 5.5 6.5"
            stroke="currentColor"
            stroke-width="1.3"
            stroke-linecap="round"
            stroke-linejoin="round"
          />
        </svg>
        哔哩哔哩
      </button>
    </div>

    <div class="deck">
      <InstallButton :state="state" @press="$emit('press')" />
    </div>
  </div>
</template>
