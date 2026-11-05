<script setup lang="ts">
import { ref } from "vue";

defineOptions({ name: "Avatar" });

const url = ref("a.png");
</script>

<template>
  <img :src="url" alt="avatar" />
</template>
