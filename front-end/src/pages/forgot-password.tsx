import { useState } from 'react'
import { Logo } from '../components/logo'
import { Button } from '../components/button'
import { Field, Label } from '../components/fieldset'
import { Heading } from '../components/heading'
import { Input } from '../components/input'
import { Strong, Text, TextLink } from '../components/text'
import { useAuth } from '../contexts/AuthContext'

export default function ForgotPassword() {
  const [username, setUsername] = useState('')
  const [serverUrl, setServerUrl] = useState('')
  const [error, setError] = useState('')
  const [isLoading, setIsLoading] = useState(false)
  const [success, setSuccess] = useState(false)
  
  const { resetPassword, setApiBaseUrl } = useAuth()
  
  const handleSubmit = async (e: React.FormEvent) => {
    e.preventDefault()
    
    if (!username || !serverUrl) {
      setError('Username and server URL are required')
      return
    }
    
    // Store the server URL for API requests
    setApiBaseUrl(serverUrl)
    
    setIsLoading(true)
    setError('')
    
    try {
      await resetPassword(username)
      setSuccess(true)
    } catch (err) {
      setError(err instanceof Error ? err.message : 'Password reset request failed')
    } finally {
      setIsLoading(false)
    }
  }
  
  return (
    <form onSubmit={handleSubmit} className="grid w-full max-w-sm grid-cols-1 gap-8">
      <Logo className="h-6 text-zinc-950 dark:text-white forced-colors:text-[CanvasText]" />
      <Heading>Reset your password</Heading>
      <Text>Enter your username and we'll send you a link to reset your password.</Text>
      
      {error && (
        <div className="bg-red-50 border border-red-200 text-red-800 px-4 py-3 rounded">
          {error}
        </div>
      )}
      
      {success && (
        <div className="bg-green-50 border border-green-200 text-green-800 px-4 py-3 rounded">
          Password reset instructions have been sent to your email.
        </div>
      )}
      
      <Field>
        <Label>Server URL</Label>
        <Input 
          type="text" 
          name="serverUrl" 
          value={serverUrl} 
          onChange={(e) => setServerUrl(e.target.value)} 
          placeholder="http://localhost:8080"
          required 
          disabled={success}
        />
      </Field>
      
      <Field>
        <Label>Username</Label>
        <Input 
          type="text" 
          name="username" 
          value={username}
          onChange={(e) => setUsername(e.target.value)}
          required
          disabled={success}
        />
      </Field>
      <Button 
        type="submit" 
        className="w-full"
        disabled={isLoading || success}
      >
        {isLoading ? 'Sending...' : 'Reset password'}
      </Button>
      <Text>
        Remember your password?{' '}
        <TextLink href="/login">
          <Strong>Sign in</Strong>
        </TextLink>
      </Text>
      <Text>
        Don't have an account?{' '}
        <TextLink href="/register">
          <Strong>Sign up</Strong>
        </TextLink>
      </Text>
    </form>
  )
}