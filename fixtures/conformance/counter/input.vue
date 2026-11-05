<script setup lang="ts">
import { ref, computed } from "vue";

const count = ref(0);
const doubled = computed(() => count.value * 2);
</script>

<template>
  <div class="counter p-4" :style="{ display: 'flex', padding: '16px' }">
    <button @click="count = count + 1">Add</button>
    <span>{{ count }}</span>
    <p v-if="doubled > 10" class="hot">Big: {{ doubled }}</p>
  </div>
</template>
