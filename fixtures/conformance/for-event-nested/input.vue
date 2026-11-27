<script setup lang="ts">
import { ref } from "vue";

defineOptions({ name: "Groups" });

const groups = ref([["alpha", "beta"]]);
const picked = ref("");
</script>

<template>
  <ul v-for="g in groups" :key="g.length">
    <li v-for="n in g" :key="n" @click="picked = n">{{ n }}</li>
  </ul>
</template>
