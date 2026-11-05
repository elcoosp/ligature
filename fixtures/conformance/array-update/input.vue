<script setup lang="ts">
import { ref } from "vue";

defineOptions({ name: "Cart" });

const tags = ref(["new"]);
</script>

<template>
  <div>
    <button @click="tags = [...tags, 'hot']">Tag</button>
    <b v-for="t in tags">{{ t }}</b>
  </div>
</template>
