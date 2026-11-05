<script setup lang="ts">
import { ref } from "vue";

defineOptions({ name: "Search" });

const query = ref("");
</script>

<template>
  <div class="search">
    <input :value="query" @input="query = $event.currentTarget.value" />
    <p>query: {{ query }}</p>
  </div>
</template>
