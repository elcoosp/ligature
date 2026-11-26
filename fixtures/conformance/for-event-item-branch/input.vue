<script setup lang="ts">
import { ref } from "vue";

defineOptions({ name: "Rows" });

const items = ref(["alpha", "beta"]);
const picked = ref("");
</script>

<template>
  <ul>
    <li v-for="n in items" :key="n" v-if="n.length > 4" @click="picked = n">{{ n }}</li>
    <li v-else @click="picked = 'short'">{{ n }}</li>
  </ul>
</template>
