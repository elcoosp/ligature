<script setup lang="ts">
import { ref } from "vue";

defineOptions({ name: "Badge" });

const active = ref(false);
</script>

<template>
  <span :class="active ? 'badge on' : 'badge'">B</span>
</template>
