<script setup lang="ts">
import { computed, ref } from "vue";

defineOptions({ name: "Sum" });

const a = ref(2);
const b = ref(3);
const total = computed(() => a.value + b.value);
</script>

<template>
  <output>{{ total }}</output>
</template>
