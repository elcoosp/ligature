<script setup lang="ts">
defineOptions({ name: "Greeting" });

const { name, greeting } = defineProps<{ name: string; greeting: string }>();
</script>

<template>
  <h1>{{ greeting }} {{ name }}</h1>
</template>
